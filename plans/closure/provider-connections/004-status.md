# Provider Connections Milestone 004 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/provider-connections/004-lifecycle-rotation-health-closure.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-4--rotation-health-deletion-and-closure`

Repository baseline reviewed: `213272c` (`main`; closure of Milestone 003 plus subsequent planning-only commits)

Implementation commits or pull requests:

- None. The plan was authored and registered as `ready for handoff` (commit `5fc689c`, "plans: add provider lifecycle rotation closure milestone") but no production implementation work landed against it. The closure record from Milestone 003 (`plans/closure/provider-connections/003-status.md:42`, `:358`) already recorded that rotation, health refresh, deletion, and team authorization remained Milestone 004 scope.

## 1. Executive finding

Milestone 004 is not closed. The plan's seven work packages (A through F) are unfulfilled: there is no rotation transaction, no refresh coordinator, no soft-delete/tombstone state, no restore or purge eligibility, no TUI lifecycle actions beyond selection, no end-to-end fake-daemon/CoreClient lifecycle harness, and no fix for the originally flaky provisioning test. The plan's acceptance criteria — staged rotation with atomic commit, in-flight revision pinning, bounded refresh with coalescing and backoff, lifecycle state machine with enable/disable/delete/restore/purge, selected-session lifecycle projection, typed turn-submit connection-state failures, and a deterministic fake-daemon TUI harness — are not evidenced by any production code in this milestone.

The infrastructure pieces that Milestone 004 was supposed to build upon are present and correct: the `ProviderConnectionStore` and `ConnectionManager` from Milestone 001, the `digest_models` helper and bounded probe from Milestone 002, and the `SelectionService` with stale-revision outcomes from Milestone 003. The milestone was never started; no commit, partial or otherwise, exists against it.

A corrective implementation plan is filed at `plans/implementation/provider-connections/005-corrective-lifecycle-rotation.md` and registered in `plans/registry.md`. Until that plan lands, Milestone 004 remains open and the Phase 2 roadmap exit criteria are not closed.

## 2. Requirement-to-evidence matrix

The matrix below grades each acceptance criterion from `plans/implementation/provider-connections/004-lifecycle-rotation-health-closure.md` §13 against the current `main` (`213272c`).

| # | Acceptance criterion | Evidence | Result | Notes |
|---|---|---|---|---|
| 1 | Credential/endpoint rotation is staged, probed, atomic, revisioned, and rollback-safe. | No `rotate_*` method exists on `ConnectionManager` (`src/core/provider_connections.rs:103-208`) or `ProviderConnectionStore` (`crates/codegg-core/src/provider_connections.rs:590-846`). Grep for `fn rotate\|async fn rotate\|rotate_credential\|rotate_endpoint` returns zero matches in `src/`, `crates/codegg-core/src/`, and `crates/codegg-protocol/src/`. No protocol variant `ConnectionRotate*` exists in `crates/codegg-protocol/src/core.rs:272-400`. | fail | Work package B is unfulfilled. |
| 2 | In-flight requests keep their captured revision; new requests use the new committed revision. | `ConnectionManager::resolve` already caches by `(id, revision)` via `OnceCell` (`src/core/provider_connections.rs:151-161`) so old callers keep their resolved runtime; the rotation path that would produce a new committed revision and a corresponding cache invalidation does not exist. | partial | In-flight semantics are an emergent property of the cache, not an explicit rotation guarantee. |
| 3 | Health/model refresh is bounded, cancellable, coalesced, and startup-independent. | `CoreRequest::ModelsRefresh` is declared at `crates/codegg-protocol/src/core.rs:400` but is not handled by `src/core/daemon.rs`. No `RefreshCoordinator`, single-flight, backoff, jitter, per-connection/global cap, or cancel primitive exists. Daemon startup does not probe. | partial | No startup probe — that part holds — but no bounded/coalesced refresh path either. |
| 4 | Failed refresh retains the last valid catalog/runtime. | No refresh failure handling exists; only the provisioning-time `provider_connection_health` table from Milestone 002 (`crates/codegg-core/src/session/schema.rs:1261`) persists state. | fail | No code path to preserve or restore last-good state. |
| 5 | Disable/delete/credential-missing never trigger silent fallback. | `ProviderConnectionStore::transition` and `disable` exist (`crates/codegg-core/src/provider_connections.rs:748-796`), and `ConnectionManager::resolve` rejects `Disabled`/`CredentialMissing` (`src/core/provider_connections.rs:38-39`). `SelectionService` returns `ConnectionNotSelectable` for disabled/deleted connections. `tests/session_selection.rs::disabled_connection_is_not_selectable` exercises this. | pass | This invariant already held at Milestone 003 and is preserved. |
| 6 | Selected sessions expose actionable lifecycle/model state. | `SessionSelectionDto::Selected` (`crates/codegg-protocol/src/provider.rs:172-177`) carries `connection_revision`, `catalog_revision`, and a single `SelectedModelDto`. `SelectionUpdateOutcome::StaleRevision` (`src/core/session_selection.rs:42-45`) returns `current_connection_id` and `current_revision` but no `current_selected_model_id`. There is no `ConnectionDisabled`/`ConnectionDeleted`/`CredentialMissing`/`RemovedModel` projection in the outcome variants. | partial | Disabled is surfaced via `ConnectionNotSelectable`; deleted, missing-credential, and removed-model states are not surfaced as actionable typed outcomes. |
| 7 | Soft delete/restore and purge blockers are explicit and restart-safe. | `ProviderConnectionStore::delete` is a hard row DELETE (`crates/codegg-core/src/provider_connections.rs:798-831`). There is no tombstone/soft-delete state, no `restore`, no reference counting, and no blocker enumeration. `ProviderConnectionState` has exactly three variants (`Active`, `Disabled`, `CredentialMissing` at `crates/codegg-core/src/provider_connections.rs:339-343`); the source comment at `:335-336` notes "Deleted is represented by row absence after `ProviderConnectionStore::delete_metadata`." | fail | Soft delete, restore, and purge are unfulfilled (work package A). |
| 8 | `/connections` supports lifecycle actions without exposing secrets. | `Command::new("/connections", …)` is registered at `src/tui/command.rs:102-104` and opens `Dialog::ConnectionSelection`. The dialog's keymap at `src/tui/components/dialogs/connection_selection.rs:125-153` exposes only `Esc`/`q`/`Down`/`Up`/`Tab`/`BackTab`/`Enter` (close, navigate, submit selection). No refresh, rotate, enable, disable, delete, restore, or confirmation action exists. No secret-input surface exists. | partial | Selection works and remains secret-free; no lifecycle actions. |
| 9 | End-to-end fake-daemon/TUI lifecycle coverage exists. | No test module under `tests/` matches `*lifecycle*`, `*rotation*`, `*refresh_coordinator*`, `*connection*`. The only related integration file is `tests/session_selection.rs` (selection only). | fail | Work package E is unfulfilled. |
| 10 | The provisioning timing test is deterministically stabilized. | `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` still exists unchanged at `src/core/eggpool.rs:1077-1122` using `#[tokio::test(flavor = "current_thread")]` and `fake_eggpool(Duration::ZERO)`. Synchronization is via `server.join()` on a `std::thread::JoinHandle` at `:1095`; no oneshot/barrier/Notify was added. | fail | Work package F is unfulfilled. |
| 11 | All Phase 2 roadmap exit criteria have closure evidence. | Milestone 003 closure did not advance the subsystem roadmap's Phase 2 conclusion. Phase 2 exit conditions in `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool` that depend on rotation, deletion-reference policy, and lifecycle-correctness are not evidenced. | fail | Phase 2 remains open. |

Headline: 0 fully pass, 4 partial (existing infrastructure that partially meets a criterion), 7 fail.

## 3. Production implementation evidence

### What exists from prior milestones and is reused

- **`ProviderConnectionStore`** (`crates/codegg-core/src/provider_connections.rs:590-846`) — additive migrations v24 (`provider_connections` table) and v26 (`provider_provisioning`, `provider_connection_health`, `provider_connection_models` tables) per `crates/codegg-core/src/session/schema.rs:1052-1298`. Revision-safe `update` and `transition` with optimistic locking; `disable` shorthand at `:789-796`; `delete` (hard row delete) at `:798-831`.
- **`ConnectionManager`** (`src/core/provider_connections.rs:94-208`) — lazy resolution, `OnceCell` cache keyed by `(connection_id, revision)`, `invalidate(connection_id)` at `:183-186` and `invalidate_revision(connection_id, revision)` at `:189-195` already provide the in-flight-pinning primitive that Milestone 004 requires rotation to drive.
- **`digest_models`** (`crates/codegg-providers/src/eggpool.rs:511-518`) — SHA-256 digest of sorted `(id, name)` model rows; used as the catalog revision during initial provisioning. Currently private and consumed only by `probe_eggpool_models` (`:507`) and an internal test (`:665`).
- **`SelectionService`** (`src/core/session_selection.rs:38-64, :507+`) — five-outcome update protocol including `StaleRevision`, `StaleCatalog`, `ConnectionNotSelectable`, `UnknownModel`, and `Updated`. `ConnectionNotSelectable` carries the disabled/deleted state string and prevents silent fallback.
- **`SessionSelectionDto`** (`crates/codegg-protocol/src/provider.rs:168-184`) — `Selected` variant carries `connection_revision` and `catalog_revision`. Currently annotated `#[allow(clippy::large_enum_variant)]` at `:170`; the milestone's "box or split the large variant rather than adding more unboxed payload weight" guidance was not applied.
- **`/connections` dialog** (`src/tui/components/dialogs/connection_selection.rs:23-282`) — selection-only TUI; secret-free by construction.
- **Server secret-transport denial** (`src/server/ws.rs:930-941`) — guards `CoreRequest::EggpoolConnectionCreate` against remote WebSocket carriage of secrets. A future `ConnectionRotate` variant would need to be added to the matches! arm; this is not a defect of Milestone 004 because the variant does not yet exist.

### What is missing

- **No rotation transaction.** Grep across `src/` and `crates/codegg-core/src/` returns zero matches for `fn rotate`, `rotate_credential`, `rotate_endpoint`, `staged_credential`, `staged_binding`, or `expected_revision` in a rotation context. `ConnectionManager` has no `rotate_*` method.
- **No refresh coordinator.** No `RefreshCoordinator`, no single-flight primitive keyed by `ProviderConnectionId`, no backoff/jitter calculation for provider health, no per-connection or global cap. `CoreRequest::ModelsRefresh` (`crates/codegg-protocol/src/core.rs:400`) is declared but has no handler.
- **No soft delete, restore, or purge.** `ProviderConnectionState` has exactly three variants at `crates/codegg-core/src/provider_connections.rs:339-343`. `delete` is a hard row DELETE with no tombstone row and no restore path. There is no reference counter, no blocker enumerator, and no purge-eligibility check.
- **No session lifecycle projection.** `SelectionUpdateOutcome` carries five variants but none project "disabled/deleted/credential-missing/removed-model" states back to the client with current selection fields. Stale outcomes carry `current_revision` and `current_catalog_revision` but not `current_selected_model_id`.
- **No TUI lifecycle actions.** The `/connections` dialog has no keybinding, command, or surface for refresh, rotate, enable/disable, delete/restore, confirmation, or blocker rendering.
- **No fake-daemon TUI harness.** No test in `tests/` matches `*connection*`, `*lifecycle*`, `*rotation*`, or `*refresh*`.
- **No fix for the flaky provisioning test.** The test at `src/core/eggpool.rs:1077-1122` is unchanged from Milestone 002.
- **No architecture doc updates** for `architecture/provider.md`, `architecture/auth.md`, `architecture/session.md`, `architecture/protocol.md`, `architecture/storage.md`, `architecture/tui.md`, or `architecture/core.md` describing Milestone 004 semantics.

### Storage schema delta

None. `STORAGE_LAYOUT_VERSION = 28` (`crates/codegg-core/src/storage/mod.rs:39`). The most recent migration, `migrate_v28` (`crates/codegg-core/src/session/schema.rs:1324`), adds Project Catalog M1 columns and is not a Milestone 004 deliverable.

## 4. Verification executed

Per the planning process, the closure record reports what was actually run and observed rather than an aspirational verification log. The verification commands listed in the plan's §11 were not executed for Milestone 004 because no production changes were made. The commands that *can* be run cleanly against current `main` are listed for the corrective plan's reference:

```bash
rtk cargo check --workspace --all-features
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo test -p codegg-providers
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-core session::legacy_resolution
rtk cargo test -p codegg-protocol
rtk cargo test --test session_selection
rtk cargo test -p codegg --lib core::provider_connections
rtk cargo test -p codegg --lib core::eggpool
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

The corrective plan at `005-corrective-lifecycle-rotation.md` carries the full verification suite and the formerly flaky-test repeated-load loop as acceptance evidence.

### Static-guard status

- `scripts/check-core-boundary.sh`: pass (pre-existing; nothing in this milestone touched `codegg-core`).
- `scripts/check_daemon_cwd_usage.py`: pass (pre-existing).
- `scripts/check_execution_ownership.py`: pass (no new process-spawn site added).
- `scripts/check_scheduler_bypass.py`: pass (no scheduler bypass introduced).
- `scripts/check_git_forbidden_patterns.py`: not in scope for this milestone.

## 5. Invariant review

The plan's §4 invariants remain satisfied by the prior milestones and were not regressed because this milestone landed no code:

- Plaintext credentials confined to bounded secret-input memory and protected credential-store operations — preserved.
- Secrets never enter SQLite, protocol DTOs, TUI snapshots, logs, diagnostics, project config, chat, or audit metadata — preserved.
- Connection ID and provider implementation ID remain distinct — preserved.
- In-flight requests retain the provider instance/revision captured at request start — preserved by the existing `OnceCell` cache key, but rotation does not exist to produce a new committed revision.
- New requests use the latest successfully committed active revision — preserved for unrotated connections; no rotation path exists to violate or extend this invariant.
- Disable/delete never silently selects another connection — preserved by `SelectionService` (`ConnectionNotSelectable`) and `ConnectionManager::resolve` error variants.
- Health/model probing remains explicit or bounded — startup does not probe; no refresh coordinator exists to violate this.
- Scope metadata does not grant authorization — preserved; scope remains metadata-only.

No invariant is regressed by this milestone's lack of implementation. The corrective plan must not regress any of these on its path to closure.

## 6. Failure and recovery review

| Failure or scenario | Observed behavior today | Required by plan | Gap |
|---|---|---|---|
| Operator attempts to rotate a credential | No rotation entrypoint exists. The closest action is `ProviderConnectionStore::disable`, which leaves the active binding in place and prevents new resolutions. | Staged credential, bounded probe, atomic commit, typed errors. | Work package B unfulfilled. |
| Operator attempts to refresh a model catalog | `CoreRequest::ModelsRefresh` is declared but unhandled; the daemon returns an `Error` response. | Bounded refresh with single-flight, backoff, jitter, last-good preservation. | Work package C unfulfilled. |
| Operator deletes a connection with active sessions | `ProviderConnectionStore::delete` is a hard row DELETE; subsequent `SelectionService::update` for a session selecting that connection returns `ConnectionNotSelectable` (no row). | Soft delete/tombstone with explainable identity, restore capability, purge blockers, restart-safe state. | Work package A unfulfilled. |
| Daemon restart after operator disable | `ConnectionManager` reconstructs lazily from the store; disabled connections cannot be resolved. | Lazy reconstruction of committed state; no synchronous probe. | The lazy-reconstruction half holds (Milestone 001). The probe-storm half is untested because no refresh exists. |
| Remote WebSocket client attempts to rotate with a secret | No `ConnectionRotate` variant exists, so the request is rejected as `UnknownRequest`. | Future rotate requests carrying a secret must be rejected by `src/server/ws.rs:930-941`; the matches! arm must be extended when the variant is introduced. | Work package B unfulfilled; the guard extension is a precondition for the corrective plan. |

## 7. Migration and compatibility review

- No storage migration occurred; `STORAGE_LAYOUT_VERSION` remains `28`.
- No protocol DTO additions or deprecations occurred. Additive-only posture is preserved.
- No CLI command additions or removals occurred. The 107-command ledger holds.
- No provider registry, environment variable, or `register_builtin` path was modified.
- The legacy `provider`/`model` strings and `LegacyResolution` outcomes remain the source of truth for pre-Milestone-003 sessions, as recorded in `plans/closure/provider-connections/003-status.md`.

The corrective plan must preserve all of the above and add its work additively.

## 8. Security review

The milestone's security posture is unchanged from Milestone 003 because no code landed:

- **No new credential writes.** The credential store was not touched. The local-only secret-transport guard at `src/server/ws.rs:930-941` continues to deny `EggpoolConnectionCreate` over remote WebSocket.
- **No new DTOs carrying secrets.** The protocol surface grew zero variants; the existing selection DTOs remain secret-free.
- **No new TUI surface.** The `/connections` dialog is unchanged; secret input has nowhere to enter.
- **No new endpoint validation path.** Eggpool endpoint validation from Milestone 002 (`crates/codegg-providers/src/eggpool.rs`) continues to enforce scheme/host/port/TLS/redirect policy.
- **No fallback accounts.** `ProviderConnectionStore::update` and `ConnectionManager::resolve` still operate on exact `ProviderConnectionId` lookups.

Security obligations that the corrective plan inherits and must implement:

- Reject any future rotate request carrying a credential over WebSocket by extending the matches! arm at `src/server/ws.rs:930-941`.
- Keep the rotation's staged credential out of SQLite metadata, TUI dialog state, logs, audit metadata, and protocol DTOs.
- Re-run endpoint scheme/host/port/TLS/redirect validation on every rotation that changes endpoint metadata.
- Never delete an unrelated credential-store record during rotation cleanup.
- Bound error bodies and URLs to avoid leaking secret-bearing query strings or headers.

## 9. Documentation and operations

- `AGENTS.md` ledger was not modified; 107-command count, 44-event count, 39 LSP servers, and ~37 tools remain accurate.
- No architecture document was updated. The plan §12 requires updates to `architecture/provider.md`, `architecture/auth.md`, `architecture/session.md`, `architecture/protocol.md`, `architecture/storage.md`, `architecture/tui.md`, `architecture/core.md`, and operator examples for Eggpool connect/rotate/refresh/disable/delete. None of these updates landed.
- No closure matrix or legacy-removal criteria document was produced. The plan §15 requires a complete Phase 2 requirement-to-evidence matrix.

The corrective plan carries these documentation updates as part of its scope and must produce a closure matrix as part of its closure record.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| critical | Rotation transaction does not exist. Operators cannot rotate credentials or change endpoint metadata without deleting and recreating the connection, which destroys history and references. | Phase 2 cannot close. `/connect` is the only lifecycle operation available. | Implement work package B per `005-corrective-lifecycle-rotation.md`. |
| critical | Refresh coordinator does not exist. Health and model catalogs are frozen at provisioning time and grow stale silently. | `/connections` displays the wrong model list once Eggpool changes its catalog. | Implement work package C per the corrective plan. |
| critical | Soft delete/tombstone state does not exist. `delete` is a hard row delete that orphans session references and diagnostic history. | Sessions whose connection is deleted lose explainable identity; audit trails are corrupted. | Implement work package A per the corrective plan. |
| high | No TUI lifecycle actions. `/connections` only supports selection. Operators have no in-TUI surface to refresh, rotate, enable/disable, delete, or restore. | Operators must use future protocol-level actions or out-of-band SQL; the TUI cannot deliver the milestone's user-visible capability. | Implement work package E per the corrective plan. |
| high | Flaky provisioning test `successful_provision_persists_redacted_connection_and_catalog` remains unchanged at `src/core/eggpool.rs:1077-1122`. | Reproducible timing race under broad parallel load. | Implement work package F per the corrective plan: deterministic fake-server readiness, operation completion signaling, isolated test resources. |
| high | No end-to-end fake-daemon/CoreClient lifecycle harness. | Milestone 004 cannot be regression-tested at the TUI surface. | Implement the harness as part of work package E. |
| medium | `SessionSelectionDto` large-enum-variant suppression remains; lifecycle additions would worsen it. | Clippy pedantic lint persists; future protocol additions compound. | Resolve when the corrective plan touches the DTO (box or split the `Selected` variant). |
| medium | `SelectionUpdateOutcome::StaleRevision`/`StaleCatalog` do not surface the currently selected model. | Clients must perform a separate `SessionSelectionGet` to reconcile. | Extend the outcome variants when the corrective plan revises session/protocol reconciliation. |
| medium | `src/server/ws.rs:930-941` does not guard any future `ConnectionRotate` request. | The matches! arm will let a future rotate-with-secret request through if the variant is added without extending the guard. | Extend the guard as part of work package B. |
| low | Pre-existing pedantic clippy lints in unrelated code (e.g. the TUI message enum) remain. | Cosmetic; not introduced or resolved by this milestone. | Out of scope for this milestone and for the corrective plan. |

No critical, high, or medium finding has been addressed by this closure pass because no production change was made.

## 11. Roadmap disposition

The Provider Connections and Eggpool roadmap remains **active** with Milestone 4 in the **corrective pass required** state and Milestone 5 (`plans/implementation/provider-connections/005-corrective-lifecycle-rotation.md`) registered as the corrective implementation plan.

### Disposition by plan exit criterion

| Roadmap exit criterion (roadmap §7 M4 + plan §13) | Disposition |
|---|---|
| Rotation takes effect for new requests without exposing secrets. | Open — corrective plan work package B. |
| In-flight requests have defined behavior and do not switch credentials mid-request. | Open — corrective plan work package B (semantics document + tests). |
| Deletion/disable is explicit and recoverable where policy allows. | Open — corrective plan work package A. |
| Health probes are bounded and do not overload Eggpool. | Open — corrective plan work package C. |
| All Phase 2 exit criteria are evidenced. | Open — corrective plan work package F (closure matrix). |

### Unblocking of downstream work

No blocked plan that names Milestone 004 as a hard dependency can be unblocked by this closure. Specifically:

- `plans/implementation/domain-identity/003-daemon-protocol-adoption.md` references Project Catalog Milestone 004 (the catalog protocol/server migration), which is **not** Provider Connections Milestone 004 and is a separate plan registered under the Project Catalog and Lazy Discovery subsystem. That plan's blocker is owned by Project Catalog Milestone 004 and is unaffected by this closure.
- `plans/implementation/runtime-assets/002-explicit-context-agent-instruction-resolution.md:102` references "Milestone 004" as the owner of refresh/activation semantics — that refers to Runtime Assets Milestone 004, not Provider Connections Milestone 004. Unaffected by this closure.
- No TUI, Session Projections, or other downstream plan names Provider Connections Milestone 004 as a hard dependency. The provider-connections roadmap itself is the only consumer, and its next milestone is the corrective plan filed at `005-corrective-lifecycle-rotation.md`.
- Team authorization, which Milestone 003 closure §11 deferred to a later identity/authorization phase, remains deferred and is unaffected by this closure.

### Future plan registration

The corrective implementation plan `plans/implementation/provider-connections/005-corrective-lifecycle-rotation.md` is registered as the dependency-ready follow-up. Its baseline is `213272c` (current `main`), its class is **capability**, and its dependencies are hard on the existing Milestones 001–003 infrastructure plus an interface dependency on the corrective resolution of the large-enum-variant lint and the server WS guard extension noted as required preconditions.

## 12. Registry updates

- Source plan `004-lifecycle-rotation-health-closure.md` status changes from `ready for handoff` to `corrective pass required`, pointing to this closure record and to `005-corrective-lifecycle-rotation.md`.
- Source subsystem roadmap updates Milestone 4 status to `corrective pass required`, adds a Milestone 5 entry, and links this closure record plus the corrective plan.
- `plans/registry.md` moves Provider Connections Milestone 4 from the dependency-ready table to the active closure table, adds Milestone 5 to dependency-ready, and leaves the recently-closed rows for Milestones 001–003 unchanged.
- No blocked plan is unblocked by this closure.

## 13. Handoff notes

- The plan's §16 "ready for handoff" handoff notes continue to apply to the corrective plan: the production baseline is `213272c`; preserve the existing credential store and local-only secret transport; use deterministic fake-server readiness and clocks instead of arbitrary sleeps; follow the repository's resource-conscious test configuration; and treat scope metadata as non-authorization.
- The corrective plan explicitly forbids reopening already closed scope (Milestones 001–003) without evidence.
- A repeated corrective pass without progress would indicate that the subsystem roadmap's milestone sizing should be revised; the corrective plan is sized to one implementation agent pass per `plans/003-planning-process.md` §5.
