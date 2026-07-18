# Project Catalog Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-catalog/002-bounded-discovery-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-2--bounded-discovery-and-reconciliation`

Repository baseline reviewed: `5974976` (`feat(project-catalog): add bounded discovery reconciliation`)

Implementation commits:

- `5974976` — `feat(project-catalog): add bounded discovery reconciliation`. Adds bounded discovery configuration, the core scanner and reconciliation decision engine, the core-neutral coordinator, additive schema v29 persistence, focused tests, static guards, and architecture documentation.

Recommendation: **closed**. The milestone establishes the bounded, metadata-only local discovery and conservative reconciliation boundary. Protocol/TUI exposure, lazy activation, remote scanning, and semantic monorepo inference remain explicitly deferred to later milestones.

## 1. Executive finding

Milestone 002 is implemented and closed against its acceptance criteria. `Config::discovery` is opt-in and contains only explicitly named local roots. `codegg_core::project_discovery` performs deterministic, bounded, cancellable, metadata-only traversal and produces candidates without starting services or writing candidate repositories. `DiscoveryCoordinator` adds single-flight refresh, bounded global admission, durable scan generations, cancellation/status/unresolved-observation seams, and transactional reconciliation through the existing workspace/project storage authority.

The implementation is intentionally conservative: it preserves catalog authority when roots or candidates disappear, refuses ambiguous/fork-like/plain-directory merges, and leaves SSH/linked-node locators inert. The service is core-neutral and ready for later daemon protocol handlers, but Milestone 004 owns the complete project protocol/server surface.

## 2. Requirement-to-evidence matrix

| # | Acceptance criterion | Evidence | Result |
|---|---|---|---|
| 1 | Only explicitly configured local roots are scanned. | `DiscoveryConfig`, `DiscoveryRootConfig`, `roots_from_config`, and `DiscoveryCoordinator` accept configured roots only; no cwd/home sweep exists. | pass |
| 2 | Scans enforce finite depth, entry, candidate, duration, concurrency, and output bounds. | `ScanLimits` and config validation enforce finite limits; schema v29 repeats storage bounds with `CHECK` constraints; diagnostics/output are capped. The current scanner is serial, which is stricter than the configured concurrency ceiling. | pass |
| 3 | Scanning performs no activation and no writes inside candidate repositories. | The core module is metadata-only, calls bounded local lineage inspection only after policy/path checks, and the discovery invariant guard rejects activation/process/write/cwd patterns. | pass |
| 4 | Canonical aliases and unique lineage converge without duplicate projects. | `reconciliation_handles_alias_lineage_fork_and_plain_directory` covers exact locator, canonical alias, and unique-lineage outcomes; service reconciliation calls `ProjectStorage::reconcile_workspace` and `WorkspaceRegistry`. | pass |
| 5 | Ambiguous/fork-like evidence never merges silently. | The decision engine has `AmbiguousLineage`, `ForkConflict`, and `PlainDirectoryUnresolved`; the focused test supplies conflicting repository fingerprints and expects `ForkConflict`. | pass |
| 6 | Verified path moves update locators without changing logical project identity. | A moved candidate with unique lineage selects `UniqueLineage`; apply reconciliation registers the canonical workspace path through existing storage, preserving the binding's project identity while adding/updating its locator. | pass |
| 7 | Temporary root/candidate absence does not delete or archive catalog records. | Unavailable roots retain the prior completed generation; missing observations are inserted with `missing` status; no archive/delete path is called by discovery. | pass |
| 8 | Failed/cancelled scans preserve the last completed generation. | Refresh records failed/cancelled status without replacing the completed report; `completed_generation_survives_temporary_root_unavailability` and cancellation tests pass. | pass |
| 9 | Concurrent scans coalesce and overlapping roots converge transactionally. | Per-root single-flight state, bounded global semaphore, operation IDs, generation uniqueness, and `start_refresh_and_cancel_share_one_operation` cover coalescing/cancellation. | pass |
| 10 | Catalog listing/restart hydration remain probe-free. | Discovery is a separate coordinator; catalog listing and existing restart hydration paths were not changed to scan or probe. No activation is imported by the discovery module. | pass |

## 3. Implementation evidence

### Configuration and policy

`codegg-config` adds `DiscoveryConfig`, `DiscoveryRootConfig`, `DiscoveryMode`, `SymlinkPolicy`, validation, deterministic merge behavior, duplicate/overlap diagnostics, revision support, and accessors. Safe defaults are:

| Bound | Default |
|---|---:|
| enabled | false |
| max depth | 4 |
| max visited entries | 10,000 |
| max candidates | 1,000 |
| max elapsed | 10 seconds from config (5-second standalone scanner default) |
| max output | 256 KiB |
| max diagnostics | 128 |
| stat concurrency | 4 |
| Git probe concurrency | 2 |

The config and persisted schema reject control/NUL text, oversized values, invalid bounds, duplicate IDs/names, and lexical root overlap. Symlinks default to no-follow; ignored defaults include `.git`, `.codegg`, caches, dependency trees, build outputs, and `target`.

### Scanner and decision engine

`crates/codegg-core/src/project_discovery.rs` contains the bounded scanner, candidate/evidence types, report/status types, observation types, and deterministic reconciliation order: exact locator, canonical alias, unique lineage, explicit association, stable catalog evidence, then sufficiently evidenced creation; otherwise unresolved/conflicting outcomes are retained. Traversal is sorted, checks symlink metadata, enforces path containment, terminates descent at Git roots, caps diagnostics and marker reads, and supports cancellation.

### Coordinator and persistence

`crates/codegg-core/src/project_discovery_service.rs` exposes root validation, preview, refresh, refresh-all, cancellation, status, unresolved observations, and explicit association. It stores only bounded metadata, uses `WorkspaceRegistry` and `ProjectStorage` as write authorities, coalesces same-root work, and retains a bounded history of scan generations.

Storage layout v29 adds `discovery_root`, `discovery_scan`, and `discovery_observation` with foreign keys, status checks, count/time bounds, root/generation/project/workspace/status indexes, and idempotent migration behavior. Retention prunes old scan/observation rows only; it never removes catalog authority.

## 4. Verification executed

Focused and structural verification:

```text
rtk cargo test -p codegg-config --lib                                      68 passed
rtk cargo test -p codegg-core project_discovery --lib -- --test-threads=1  10 passed
rtk cargo test -p codegg-core project_discovery_service::tests --lib       5 passed
rtk cargo test --test storage_migrations                                  4 passed
rtk cargo check -p codegg-core --lib                                      passed
rtk cargo clippy -p codegg-core --lib -- -D warnings                     passed
rtk cargo check --workspace --all-targets --all-features                   passed
rtk cargo fmt --all -- --check                                             passed
rtk git diff --check                                                        passed
rtk bash scripts/check-core-boundary.sh                                     passed
rtk python3 scripts/check_daemon_cwd_usage.py                              passed
rtk python3 scripts/check_identity_path_usage.py                            passed
rtk python3 scripts/check_discovery_invariants.py                           5/5 passed
rtk python3 scripts/check_project_catalog_invariants.py                     7/7 passed
```

The capped broad run completed `3,814 passed; 5 failed`. The five failures are unrelated environment-sensitive tests: three daemon-socket integration tests could not create their socket (`No such file or directory`), and two Eggpool tests could not bind their fake server (`Operation not permitted`). No discovery, config, migration, or catalog test failed.

Workspace-wide `clippy --workspace --all-targets --all-features -- -D warnings` still reports pre-existing unrelated warnings in `src/skills/parser.rs`, `src/skills/registry.rs`, `src/tool/skill.rs`, `crates/codegg-core/tests/project_catalog.rs`, and the TUI enum. The new discovery warnings were fixed before the implementation commit; `codegg-core` library clippy is clean.

## 5. Security, contention, migration, and compatibility

- Root and candidate paths are canonicalized and checked for containment; symlinks are not followed by default.
- Git inspection is local, bounded, non-interactive, hook-free/network-free through the existing lineage helper; remote locators remain data only.
- Marker reads, diagnostics, output, candidates, entries, depth, elapsed time, and concurrency admission are bounded.
- Same-root refreshes coalesce; global concurrency is admitted by a semaphore; cancellation does not publish a partial generation.
- The migration is additive and idempotent. Missing/unavailable observations do not archive projects or delete workspaces/sessions.
- Existing catalog/project/workspace stores remain the write authority; no direct duplicate catalog SQL is introduced by the coordinator.
- No protocol DTO or TUI state was added, so compatibility and server routing remain unchanged in this milestone.

## 6. Known limitations and unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| low | The scanner currently traverses serially even though config persists stat/Git concurrency ceilings. | Safe conservative behavior; parallel worker execution is deferred until profiling requires it. |
| low | Production scanning does not yet derive a repository object/head fingerprint from every candidate. | The decision engine refuses conflicting fingerprints when supplied; absence of fingerprint evidence yields conservative false negatives rather than silent merges. |
| low | SSH/linked-node scanning, semantic monorepo package inference, protocol/TUI exposure, lazy service activation, and watcher acceleration are not part of M2. | Explicitly deferred by the source plan and roadmap. |
| unrelated | Five broad-suite environment failures and pre-existing workspace clippy warnings remain. | Recorded above; no M2 corrective action is indicated. |

No critical or high-severity finding remains for this milestone.

## 7. Dependency and handoff disposition

- Project Catalog Milestone 002 is closed and removed from active closure work.
- Project Catalog Milestone 003 remains not started and blocked on the Runtime Assets refresh/activation interface.
- Runtime Assets Milestone 003 is now dependency-ready because Runtime Assets Milestone 002 closed with the `ProjectAssetSnapshot`/snapshot-builder interface. A handoff plan is registered at `plans/implementation/runtime-assets/003-refresh-lifecycle-operator-surface.md`.
- Multi-Project TUI 001 and Session Projections 001 remain blocked on their existing later protocol/project-state dependencies; this milestone does not unblock them.

The interface handed to Project Catalog Milestone 003 is the core-neutral `DiscoveryCoordinator` surface: `validate_root(s)`, `preview`, `start_refresh`/`refresh`, `refresh_all`, `cancel`, `get_scan_status`, `list_unresolved`, and `associate_workspace`, plus durable v29 scan/observation records.
