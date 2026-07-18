# Project Catalog Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-catalog/003-lazy-activation-and-health.md`

Source subsystem roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-3--lazy-activation-and-health`

Repository baseline reviewed: `5f65c67` (`docs(plans): close runtime-assets milestone 004`); source-plan baseline `972c286`

Implementation commits:

- `27cbd43` — `feat(project-catalog): add lazy activation and health`. Adds the daemon-owned activation registry, bounded owner leases, workspace-service lease-accounting fixes, refresh/health integration, focused tests, and architecture documentation.

Recommendation: **closed**. Project Catalog Milestone 003 satisfies its infrastructure exit conditions. Milestone 004 is now dependency-ready; its implementation handoff has not yet been authored or registered. Multi-Project TUI 001 and Session Projections 001 remain blocked on that later protocol/project-state work.

## 1. Executive finding

Milestone 003 is implemented and closed against its acceptance criteria. Catalog listing and read-only health remain probe-free, while explicit activation acquires a bounded owner-scoped lease for one project/workspace scope, refreshes assets through the existing Runtime Assets coordinator seam, and exposes a bounded aggregate health snapshot. Same-owner activation is idempotent and concurrent requests coalesce; distinct scopes are isolated and bounded by a registry-wide capacity limit. Expiry and release cleanly return workspace services to the existing idle-eviction authority.

The implementation is deliberately infrastructure-only. It does not add the M004 project protocol/server migration, TUI tabs, remote execution, authorization, or a second asset refresh coordinator. Durable catalog health remains an operator/catalog concern; activation health is a transient aggregate so a stale runtime observation cannot poison the durable project-health row.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Listing many catalog records retains no service leases and performs no expensive activation | `tests/project_activation.rs::catalog_listing_is_probe_free_and_activation_is_lazy`; existing `ProjectCatalog` list/get service; project-catalog invariant guard | pass | Two projects are listed with zero active workspace-service bundles before explicit activation. |
| Activation is explicit, project/workspace-scoped, idempotent, and bounded | `src/core/project_activation.rs`; `CoreDaemon::activate_project_workspace`; unit tests for owner reuse, expiry, bounded identifiers, and capacity | pass | Same owner/key returns one lease identity; leases have a finite TTL and a registry-wide cap. |
| Only the selected workspace is activated and projects remain isolated | `tests/project_activation.rs::two_project_activation_scopes_are_isolated` | pass | Two concrete roots receive separate bundles; inactive health reports service unavailability without activation. |
| Idle eviction and clean release preserve durable catalog/session history | `tests/project_activation.rs::activation_refreshes_assets_and_is_idempotent_per_owner`; `tests/workspace_services_isolation.rs`; workspace-service lease accounting tests | pass | Dropping/expiring activation releases the underlying service lease; idle eviction remains workspace-service-owned. |
| Activation uses the existing Runtime Assets refresh seam and reports usable outcomes | `CoreDaemon::refresh_project_activation`; `asset_health_layer`; `tests/project_activation.rs::activation_rejects_refresh_without_usable_generation_and_releases_lease`; existing asset-refresh tests | pass | Published generations are used; invalid/cancelled/no-generation outcomes are surfaced and activation without a usable generation fails. |
| Health distinguishes catalog, workspace, asset, and service state with bounded path-free diagnostics | `ProjectHealthSnapshot`, `HealthLayer`, `aggregate_health`, and health redaction tests in `src/core/project_activation.rs` | pass | Precedence includes contention/stale/unavailable/error; diagnostic text is bounded and path-like content is redacted. |
| Contention coalesces and capacity is enforced without races | Same-owner concurrent integration test; workspace-service single-flight tests; registry capacity stress test across eight distinct keys; asset-refresh coalescing tests | pass | The registry serializes cap admission while preserving per-key coalescing. |
| Restart hydrates durable metadata without eagerly reactivating services | `tests/project_activation.rs::restart_hydrates_catalog_and_asset_metadata_without_activation` | pass | Catalog/workspace and asset metadata are available after restart; activation and service bundles remain empty. |
| Malformed and untrusted activation/health input is bounded | Identifier/owner limits, bounded health fields, path-free aggregation, resolver-based workspace context, and static path/ownership guards | pass | No process cwd inference, secret-bearing diagnostics, or unbounded path fields were added. |
| Architecture and operational ownership are documented | `architecture/project_catalog.md`; `architecture/workspace_services.md`; implementation plan status and this closure record | pass | The explicit activation, refresh, health, release, expiry, and restart boundaries are documented. |

## 3. Production implementation evidence

`ProjectActivationRegistry` is the daemon-owned runtime authority for transient project/workspace activation. It composes with the existing `WorkspaceServiceRegistry`, uses per-scope locks for same-owner idempotence and single-flight behavior, uses a global admission lock for the active-lease cap, and returns owner-scoped leases with finite expiry and explicit release. Expired leases evict the underlying workspace-service lease even if stale handles are still held.

`CoreDaemon` now exposes explicit activation and read-only health operations. Activation resolves the typed project/workspace binding, acquires the selected workspace service, invokes the existing `refresh_project_activation` asset seam, rejects refreshes with no usable generation, and returns a transient aggregate health snapshot plus binding revision/diagnostics. Health lookup does not activate a workspace and uses `peek` for service state.

The existing workspace-service reload path was corrected so replacement bundles do not inherit active leases belonging to an old immutable bundle and do refresh recency before idle eviction. This preserves the service registry's authority and prevents phantom lease protection or immediate post-reload eviction.

No protocol DTO/request/event, REST/WS route, TUI state, remote execution, authorization, watcher, LSP/index/build startup, or alternate asset coordinator was added. Those remain M004 or later scope.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo check -p codegg --lib
rtk cargo clippy -p codegg --lib --all-features -- -D warnings
rtk env CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
rtk cargo test -p codegg --lib core::project_activation -- --test-threads=1
rtk cargo test --test project_activation -- --test-threads=1
rtk cargo test --test workspace_services_isolation -- --test-threads=1
rtk cargo test -p codegg-core project_catalog -- --test-threads=1
rtk cargo test -p codegg-core workspace_services -- --test-threads=1
rtk git diff --check
rtk scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_discovery_invariants.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk bash scripts/check_provider_connections_m4_coverage.sh
rtk bash scripts/check_provider_connections_tombstone_compat.sh
rtk python3 scripts/check_builtin_agents.py
rtk python3 scripts/generate_builtin_agents.py --check
rtk python3 scripts/check-tokio-test-flavors.py
```

### Results

- Formatting, diff hygiene, workspace all-target/all-feature check, targeted check, and targeted clippy passed.
- The capped all-features workspace suite completed with exit 0. The changed root unit suite reported `3,833 passed; 0 failed`; all subsequent workspace unit, integration, native-crate, LSP, and doc-test suites in the command also passed.
- Focused project activation results: 5 unit tests, 6 integration tests, 11 workspace-service isolation tests, 11 project-catalog tests, and 8 workspace-service tests passed.
- Core-boundary, daemon-CWD, project-agent-PWD, discovery, project-catalog, scheduler-bypass, execution-ownership, Git-forbidden-pattern, and provider lifecycle/tombstone guards passed.
- `check_builtin_agents.py` and `generate_builtin_agents.py --check` report one pre-existing mismatch for the `general` built-in prompt between TOML and generated Rust. `check-tokio-test-flavors.py` reports 758 pre-existing bare annotations; the new project activation tests use explicit flavors and add no finding. These are unrelated repository hygiene findings and do not affect this milestone's production boundary.

## 5. Invariant review

- **Probe-free listing:** catalog list/get and read-only health do not acquire workspace services, start processes, or perform repository probes.
- **Explicit bounded activation:** only a caller-provided project/workspace/owner tuple can create an activation; identifiers, diagnostics, TTL, and active scope count are bounded.
- **Scope isolation:** workspace-service bundles are keyed by the selected canonical workspace and are never inferred from process cwd.
- **Asset refresh authority:** activation calls the existing daemon refresh seam and consumes its published/retained/invalid/cancelled outcomes; no duplicate coordinator or publication path exists.
- **Health safety:** layer precedence distinguishes stale, unavailable, contention, and error; aggregate fields are bounded and path-free.
- **Release and eviction:** lease drop/expiry releases the underlying workspace-service lease, while the existing workspace registry controls idle eviction and forced shutdown.
- **Restart hydration:** durable catalog/workspace/asset metadata hydrates independently of transient activation and service bundles.

## 6. Failure and recovery review

- **Duplicate delivery/idempotency:** concurrent same-owner activation requests coalesce to one lease identity; repeated acquisition increments a bounded handle count and remains owner-scoped.
- **Capacity race:** distinct-key admission is serialized and covered by a stress test holding successful leases across the capacity check; the configured cap is not exceeded.
- **Refresh failure:** invalid, cancelled, or no-generation refresh results are returned as health outcomes; activation with no usable generation releases its lease and fails rather than reporting false success.
- **Lease expiry/stale handle:** expiry evicts the scope and releases workspace services even when an old handle remains; later release is safe and idempotent.
- **Reload race:** replacement workspace-service bundles start with fresh active-lease accounting and refreshed recency because old leases refer to the old immutable snapshot.
- **Daemon restart:** restart rehydrates durable metadata but creates no active project activation or workspace-service bundle until explicitly selected.
- **Malformed input:** oversized owner/project/workspace identifiers and health text are rejected or bounded; diagnostic paths are redacted.
- **Cancellation and external service scope:** this milestone does not start external services during listing and does not add a new process ownership path; existing workspace-service lifecycle and execution-ownership guards remain authoritative.

## 7. Migration and compatibility review

- No database schema migration is required. The implementation uses existing project/workspace/catalog and asset metadata stores.
- Durable project health is not overwritten by transient activation aggregation, preserving existing operator-set health semantics and avoiding self-referential stale persistence.
- Existing `WorkspaceServiceRegistry` and `CoreDaemon::refresh_project_activation` APIs remain the ownership seams; activation composes with them rather than replacing them.
- Existing project/catalog/workspace restart hydration and compatibility paths remain intact. No protocol or server route changed, so M004 owns the future DTO/REST/WS migration.
- The new registry is transient and restart-safe by construction. Rollback removes the activation facade without requiring durable cleanup.

## 8. Security review

- Activation requires explicit typed project/workspace resolution and does not infer authority from cwd or arbitrary path text.
- Owner and identifier bounds, lease caps, health diagnostic caps, and finite TTL limit resource exhaustion and retained state.
- Health aggregation sanitizes slash/backslash-containing diagnostics, preventing local paths and path-like sensitive details from crossing the health surface.
- Activation/listing does not invoke shell commands, providers, plugins, LSP, indexers, or network services; the existing asset refresh seam remains daemon-owned.
- Core-boundary, daemon-CWD, execution-ownership, discovery, project-catalog, and Git secret-boundary guards passed.
- No authorization model is added in this infrastructure milestone; team authorization remains explicitly deferred to later project protocol/server work.

## 9. Documentation and operations

Updated:

- `architecture/project_catalog.md` — activation ownership, lease/TTL/capacity behavior, health aggregation, restart, and deferred M004 boundary.
- `architecture/workspace_services.md` — project activation composition and workspace-service lease/reload ownership.
- `plans/implementation/project-catalog/003-lazy-activation-and-health.md` — closed status and closure link.
- `plans/subsystems/project-catalog-roadmap.md` — M003 closed and M004 ready for handoff.
- `plans/registry.md` — M003 moved to recently closed and downstream status reviewed.

Operational callers can invoke `CoreDaemon::evict_project_activation_leases` as a bounded maintenance seam. Workspace idle eviction and shutdown remain owned by `WorkspaceServiceRegistry`; the M004 protocol/server plan will decide how project activation and health are exposed to clients.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Built-in agent asset checks report a pre-existing `general` prompt mismatch, and the Tokio flavor guard reports 758 pre-existing bare annotations. | Repository hygiene checks remain red independently of Project Catalog M003; no activation, health, or catalog behavior is affected. | Reconcile generated agent assets and migrate/allowlist the existing Tokio test inventory in separate maintenance work. |
| low | No protocol/server consumer or recurring maintenance scheduler is added in M003. | Clients cannot yet use project activation through the native protocol, and eviction requires an operational caller. | Author/register Project Catalog M004 for protocol/server migration and its maintenance integration. |

No medium, high, or critical finding remains in this milestone's implemented scope. The low findings are explicitly deferred by the source plan or are unrelated baseline hygiene conditions.

## 11. Roadmap disposition

Project Catalog Milestone 003 is closed. Project Catalog Milestone 004 is now dependency-ready because Milestones 1–3 are closed and the Runtime Assets activation-refresh interface is closed. The M004 implementation handoff is ready to be authored and registered, but no M004 plan was invented as part of this closeout.

Multi-Project TUI 001 remains blocked on Project Catalog M004's protocol/server migration. Session Projections 001 remains blocked on Project Catalog M004 and project-aware TUI state. Those blockers are intentionally unchanged.

## 12. Registry updates

Applied in this closeout:

- Marked the source implementation plan `closed` with a link to this record.
- Marked Project Catalog M003 closed in the subsystem roadmap and moved the roadmap's current milestone to M004.
- Removed M003 from the dependency-ready implementation-plan table.
- Added M003 to recently closed work with implementation commit `27cbd43`.
- Marked M004 ready for handoff in the registry without registering an un-authored plan.
- Kept Multi-Project TUI 001 and Session Projections 001 in the blocked-work table because M004 and project-aware TUI state are not yet complete.
