# Project Catalog Milestone 002 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/project-catalog/002-bounded-discovery-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-2--bounded-discovery-and-reconciliation`

Repository baseline reviewed: `4e974f0` (`main`; closure of Provider Connections Milestones 003/004 plus planning-only commits since the plan's `3ce0a7e` baseline — the only `feat:` since baseline is `e8934d1` for Runtime Assets M2, unrelated to discovery)

Implementation commits or pull requests:

- **None.** The plan was authored at `1ccc31a` ("plans: add bounded project discovery milestone") and registered as `ready for handoff` at `cde4958` ("plans: register project catalog milestone 002"), but was never handed to an implementation agent. No `feat:`, `fix:`, or `chore:` commit in `crates/codegg-core/src/project_catalog.rs`, `crates/codegg-protocol/src/`, `src/`, `tests/`, `scripts/`, or `architecture/` corresponds to this milestone. The Milestone 001 closure record (`plans/closure/project-catalog/001-status.md`) already noted in §3 that "no configured-root scanner, scan-state store, discovery generation, candidate record, incremental reconciliation report, or daemon discovery coordinator exists"; this remains true after nine subsequent planning-only commits.

## 1. Executive finding

Milestone 002 is not closed. The plan's six work packages (A through F) are unfulfilled: there is no `DiscoveryConfig`, no `DiscoveryRoot` records, no bounded scanner, no candidate reconciliation engine, no scan generation/observation/retention store, no coordinator service seam, and no M2-specific architecture docs or static guards. The plan's ten acceptance criteria — bounded root configuration, finite scan limits, no-activation metadata-only scans, canonical-alias/unique-lineage convergence, fork/ambiguity refusal-to-merge, verified-move locator updates, temporary-absence non-deletion, generation preservation on cancellation, concurrent-scan coalescing, and probe-free catalog listing — are not evidenced by any production code in this milestone.

The infrastructure substrate the milestone was supposed to build upon is closed and correct: `codegg_core::project_catalog` (closed in `a2db5e4`), `codegg_core::project_storage::ProjectStorage` and the bounded local `repository_lineage` (closed in `84d92f0`), and the source-aware runtime-asset discovery pattern that informs M2's source-classification guidance (closed in `f9db5c3`, snapshot re-published in `e8934d1`). The milestone was never started; no commit, partial or otherwise, exists against it.

A corrective implementation plan must be authored under `plans/implementation/project-catalog/` (a successor to this plan, e.g. `003-bounded-discovery-corrective.md`, or this same plan re-handed) referencing this closure record. Until that plan lands, Milestone 002 remains open and the project catalog roadmap exit criteria for discovery, reconciliation, and consolidation remain unsatisfied.

## 2. Requirement-to-evidence matrix

The plan's §13 acceptance criteria (`plans/implementation/project-catalog/002-bounded-discovery-reconciliation.md:464-473`) are transcribed into rows and graded against the current `main` (`4e974f0`).

| # | Acceptance criterion (plan §13) | Current evidence | Result | Notes |
|---|---|---|---|---|
| 1 | Only explicitly configured local roots are scanned. | `grep -rn "DiscoveryConfig\|DiscoveryRoot\|configured_root\|discovery_config" crates/ src/` returns zero matches. The `codegg_core::project_catalog` module-doc at `crates/codegg-core/src/project_catalog.rs:8-10` explicitly states "It does NOT perform filesystem scanning." No scanner reads any config field anywhere in the daemon. | fail | Work package A unfulfilled; the discovery-config schema does not exist. |
| 2 | Scans enforce finite depth, entry, candidate, duration, concurrency, and output bounds. | `grep -rn "max_depth\|max_entries\|max_candidates\|max_duration\|max_concurrency\|max_output\|scan_bounds\|BoundedScan" crates/codegg-core/src/project_catalog.rs` returns zero matches. The only `max_depth` hits in the workspace are unrelated (`egglsp`, `codegg-config`, `agent/worker`, `tui`). | fail | Work package B unfulfilled; no scanner, no bounds type. |
| 3 | Scanning performs no activation and no writes inside candidate repositories. | No scanner code path exists. The catalog module docstring at `crates/codegg-core/src/project_catalog.rs:8-10` confirms it "does NOT perform filesystem scanning" and lists only CRUD/legacy-association operations. The Milestone 001 closure record §3 already documented this. | n/a | Vacuously true for absent scanner; does not satisfy criterion as an affirmative guarantee. Recorded as `n/a` because the criterion cannot be violated by absence. Corrective plan must establish the affirmative guard. |
| 4 | Canonical aliases and unique lineage converge without duplicate projects. | No `alias_match`, `unique_lineage`, or reconciliation method exists in `project_catalog.rs`. The only reconcile-shaped code is `ProjectStorage::reconcile_workspace_path` (Domain Identity 002, `crates/codegg-core/src/project_storage.rs`) which is bounded to one workspace passed by the caller, not to scan-collected candidates. | fail | Work package C unfulfilled; candidate reconciliation engine absent. |
| 5 | Ambiguous/fork-like evidence never merges silently. | No `fork_match`, `ambiguous_*` discovery-code path exists. The only ambiguity-bearing string in the catalog module is `'legacy_catalog_ambiguous'` at `crates/codegg-core/src/project_catalog.rs:1387`, written by Milestone 001's `conservative_legacy_association` for legacy workspace binding, not discovery reconciliation. | fail | Work package C unfulfilled; ambiguity outcome type absent. |
| 6 | Verified path moves update locators without changing logical project identity. | `grep -rn "update_locator\|path_move" crates/ src/ tests/ scripts/` returns zero matches. `ProjectCatalog::attach_locator` and `detach_locator` (Milestone 001, project_catalog.rs) operate on a project/workspace + locator tuple and have no move-detection logic; they reject identical locators rather than tracking equality across generations. | fail | No move detection exists; cannot fail by absence, but the affirmative invariant is unproven. |
| 7 | Temporary root/candidate absence does not delete or archive catalog records. | No absence-handling code exists. `ProjectCatalog::archive_project` (`project_catalog.rs:M1`) requires an explicit operator-driven source; there is no automatic archive-on-missing logic anywhere. There is also no discovery-scanner that could observe an absence. | fail | Vacuously true (no archive is triggered), but the affirmative guarantee is unproven. Corrective plan must establish the absence-preserves-catalog contract via store-level invariants. |
| 8 | Failed/cancelled scans preserve the last completed generation. | `grep -rn "scan_generation\|scan_run\|last_completed_generation" crates/ src/` returns zero matches in project-catalog code. The `cancel` hits in `crates/codegg-core/src/jobs/store.rs` are part of the durable jobs (Phase 4) subsystem, scoped to jobs/attempts/schedules, not to discovery. | fail | Work package D unfulfilled; no scan-state store exists. |
| 9 | Concurrent scans coalesce and overlapping roots converge transactionally. | `grep -rn "coalesce\|single_flight\|overlapping_root" crates/codegg-core/src/ crates/codegg-protocol/src/ src/` returns zero matches in project-catalog or discovery code. The `coalesce` hits are unrelated (job-scheduler, packet-channel I/O). | fail | Work package E unfulfilled; no coordinator or service seam exists. |
| 10 | Catalog listing/restart hydration remain probe-free. | `restart_hydration()` at `crates/codegg-core/src/project_catalog.rs` runs only `SELECT COUNT(*)` queries; `list_projects` and `get_project` use prepared SQL against catalog tables; no `inspect_repository_lineage`, `git`, `egglsp::*`, or `codegg-providers::*` calls appear in the module. Verified unchanged since Milestone 001 closure. | pass | The Milestone 001 guarantee holds; M2 must not regress it. |

Headline: **1 pass** (criterion 10), **0 partial**, **8 fail**, **1 n/a** (criterion 3, vacuous-by-absence). Of the ten acceptance criteria, the only one the plan asserted that is satisfied today is the one Milestone 001 already established and that the M2 plan explicitly inherits.

### 2.1 Work-package-to-evidence matrix

| Work package (plan §7) | Status | Evidence |
|---|---|---|
| A — Configuration and discovery domain | not implemented | No `DiscoveryConfig`, `DiscoveryRoot`, `DiscoveryMode`, `DiscoveryCandidate`, `DiscoveryObservation`, `DiscoveryReport`, or additive persistence. `grep -rn "DiscoveryConfig\|DiscoveryRoot\|scan_bounds\|scan_generation" crates/ src/` returns zero matches. |
| B — Bounded local scanner | not implemented | No scanner module, no deterministic traversal, no depth/entry/candidate/time/concurrency bounds, no Git/directory-mode detection. `project_catalog.rs` does only CRUD and `conservative_legacy_association`; no walker, no `read_dir` from catalog code. |
| C — Conservative reconciliation engine | not implemented | No `reconcile`, no alias/lineage matching engine, no typed ambiguity/fork outcomes, no path-move detection. Milestone 001's `conservative_legacy_association` (`project_catalog.rs:1187-1410`) is workspace-legacy-only and does not consume discovery candidates. |
| D — Incremental scan state and recovery | not implemented | No scan-generation/run/observation persistence, no candidate-diff against prior completed generation, no retention policy for old runs, no restart-safe scan-state machine. The Phase 4 jobs store (`crates/codegg-core/src/jobs/store.rs`) has its own generation concept scoped to jobs/attempts/schedules, not discovery. |
| E — Coordinator and operator seams | not implemented | No core-neutral discovery service, no single-flight primitive, no preview/refresh/cancel/status/report APIs, no bounded global scan concurrency. No protocol DTO additions for discovery. |
| F — Guards, docs, and scale closure | not implemented | No M2-specific static guard (`scripts/check_discovery_*.py` does not exist). No M2 sections in `architecture/project_catalog.md`, `architecture/project_identity_storage.md`, `architecture/storage.md`, `architecture/workspace.md`, `architecture/workspace_services.md`, or `architecture/config.md`. No scale/performance fixtures for discovery. |

### 2.2 Plan-prescribed verification commands (§11) outcome

The plan's §11 verification list (plan file lines 430-448) was executed against the current `main` to confirm that the focused prerequisites still pass and to document the broader suite outcome. The original `rtk cargo …` / `rtk python3 …` prefixes were preserved; `rtk` resolves to the repository's `rtk` wrapper. The commands are otherwise identical.

| Plan command | Status when run | Notes |
|---|---|---|
| `cargo fmt --all -- --check` | pre-existing diff unrelated to M2 | The `tests/skills_registry.rs` heredoc has a misplaced `use` import; confirmed pre-existing by inspection. Out of scope for this closure. |
| `cargo check --workspace --all-targets --all-features` | pass (previous closures) | Not re-run; assumed clean per prior closure runs at the same HEAD. |
| `cargo test -p codegg-core project_storage` | 7 passed, 220 filtered out (3 suites, 0.46s) | Milestone 002-dependency floor is green. |
| `cargo test -p codegg-core repository_lineage` | 4 passed, 223 filtered out (3 suites, 0.11s) | Milestone 002-dependency floor is green. |
| `cargo test -p codegg-core project_catalog` | 11 passed, 216 filtered out (3 suites, 0.01s) | Module unit tests; no integration M2 tests added. |
| `cargo test -p codegg-core workspace` | 15 passed, 212 filtered out (3 suites, 0.29s) | Milestone 002-dependency floor is green. |
| `cargo test -p codegg-core` | 227 passed (4 suites, 3.07s) | All Milestone 001 + 002 foundation tests pass. |
| `cargo test --test project_catalog` | **fails as written**: no test target named `project_catalog` in default-run packages | The plan's command was authored when the integration file did not yet exist; the actual project-catalog integration file lives at `crates/codegg-core/tests/project_catalog.rs`. The corrected command `cargo test -p codegg-core --test project_catalog` passes 18 of 18. Recorded as a factual correction; the M2 plan's command list needs to be updated when the corrective plan is authored. |
| `cargo test --test storage_migrations` | 4 passed (1 suite, 0.31s) | Includes the v28 `project_catalog_v28_is_additive_and_idempotent` test from Milestone 001. |
| `cargo test -p codegg-git` | 356 passed, 7 ignored (2 suites, 0.03s) | The bounded-lineage substrate that M2 was supposed to consume remains green. |
| `bash scripts/check-core-boundary.sh` | `codegg-core boundary check passed` | Pre-existing guard; nothing in this milestone touched `codegg-core`. |
| `python3 scripts/check_daemon_cwd_usage.py` | `cwd usage check passed — no std::env::current_dir() in protected modules` | Pre-existing guard; nothing in this milestone added cwd-derived project context. |
| `python3 scripts/check_identity_path_usage.py` | `identity path guard seam: no forbidden ProjectId construction found` | Pre-existing guard; nothing in this milestone added path-derived identity. |
| `python3 scripts/check_project_catalog_invariants.py` | `7/7 checks passed. All project catalog invariants verified.` | Pre-existing guard from Milestone 001; not updated for M2. |
| `git diff --check` | clean | Working tree is clean; no diff pending. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | fails: 8 errors from 11 pre-existing pedantic lint warnings converted by `-D warnings` | The 11 pedantic warnings are unchanged from `4e974f0` (Provider Connections 004 closure HEAD); none touch discovery code paths because no discovery code was authored. See §10 for the full lint list. The closing CLI runs `cargo clippy --workspace --all-targets --all-features` (no `-D warnings`) which reports **0 errors, 11 warnings**, all pre-existing. |
| `CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14` | 7965 passed, 10 ignored (110 suites, 189.71s) | Broad suite is green at this run; this record does not assert that this run is reproducible in every CI run, only that the M2 plan's wide-scope verification is achievable today. The Milestone 001 closure record noted a pre-existing flaky `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` from Provider Connections 003; that flakiness remains tracked under that milestone, not duplicated here. |

The pre-existing pedantic-clippy warnings are summarized in §10.

## 3. Production implementation evidence

**None.** The repository state at the time of this closure review is identical to the plan's `§3 Current implementation evidence` description (with one restatement: the Milestone 002 plan baseline was `3ce0a7e`; the latest `main` is `4e974f0` and the only production delta in that range is `e8934d1` for Runtime Assets M2, which is not project-catalog work). No new `codegg_core` module, no new `codegg_protocol` DTO, no new service tier, no `scripts/check_discovery_*.py`, no `tests/discovery_*.rs`, no architecture-document changes specifically authored for this milestone exist.

The Milestone 002 building blocks consumed by this plan remain in place and are the only relevant infrastructure to record. They are described fully in `plans/closure/project-catalog/001-status.md` and `plans/closure/runtime-assets/001-status.md`; the salient reuse-only summary is:

- `codegg_core::identity` (`crates/codegg-core/src/identity.rs`) — typed `ProjectId`, `RepositoryId`, `WorkspaceId`, `ProjectBinding`, `SessionBinding` (closed in Domain Identity 001, `f203ed9`).
- `codegg_core::project_storage::ProjectStorage` — additive SQLite v25 with `project`, `repository`, `project_repository_binding`, `workspace_project_binding`, `session_project_binding`, and `identity_diagnostic`, plus `reconcile_workspace_path`, `reconcile_catalog`, `bind_session`, `inspect_workspace`, `rebind_workspace`, and bounded `repository_lineage` (closed in `84d92f0` per `plans/closure/domain-identity/002-status.md`).
- `codegg_core::project_catalog::ProjectCatalog` — additive SQLite v28 with `list`, `get`, `register`, `archive`, `restore`, locator/health placeholders, restart hydration with no probe, and `conservative_legacy_association` (closed in `a2db5e4` per `plans/closure/project-catalog/001-status.md`).
- `scripts/check_project_catalog_invariants.py` — 7 checks covering `project_catalog.rs` file existence, no SSH/LinkedNode path coercion, `attach_locator` workspace-binding validation, v28 migration tables, v28 migration columns, `STORAGE_LAYOUT_VERSION = 28`, and `lib.rs` re-export (closed with Milestone 001).
- Runtime Assets M1 — `crates/codegg-core/src/asset_*` and `src/skills/` source-aware discovery, established as the pattern for bounded, configurable, diagnostic-surfacing discovery that Milestone 002 was meant to mirror for project roots (closed in `f9db5c3` per `plans/closure/runtime-assets/001-status.md`). Milestone 002's `§6 Scanner` requirements are modeled on this pattern; the scanner itself does not exist yet.

What is missing from this milestone:

- **No `DiscoveryConfig` schema and no config validation.** The plan's §6 "Discovery configuration" adds a stable bounded config model; nothing equivalent is in `crates/codegg-config/src/`, `src/config.rs`, or `src/core/discovery.rs`.
- **No `DiscoveryRoot`, `DiscoveryMode`, `DiscoveryCandidate`, `DiscoveryObservation`, or `DiscoveryReport` types.** None exist in any `crates/codegg-core/src/` module.
- **No scanner.** No module imports `tokio::fs::read_dir` for the purposes of bounded directory traversal; no scanner surface could be conflated with the M1 catalog listing path because no scanner was authored. The `conservative_legacy_association` walker takes one explicit workspace and does not enumerate.
- **No reconciliation engine.** No `reconcile_*` method beyond what Milestone 001 already shipped; no candidate-matching state machine; no ambiguity outcome type.
- **No scan-state store.** No `scan_run`, `scan_generation`, `scan_observation`, or retention policy. The Phase 4 jobs storage layer exists but is scoped to durable jobs/attempts/schedules.
- **No coordinator service seam.** No protocol DTO for discovery requests; no service object (no `ProjectDiscoveryService`, `DiscoveryCoordinator`, etc.); no public-facing `core` path that exposes refresh/preview/cancel/status/report.
- **No TUI surface.** The plan explicitly defers multi-project TUI tabs; none exist. The lack of scanner and the lack of TUI surface cohere to the plan's stop-condition list.
- **No M2 static guard.** `scripts/check_discovery_*.py` does not exist; `scripts/check_project_catalog_invariants.py` was not extended.
- **No M2 architecture doc updates.** `architecture/project_catalog.md`, `architecture/project_identity_storage.md`, `architecture/storage.md`, `architecture/workspace.md`, `architecture/workspace_services.md`, and `architecture/config.md` do not contain bounded-discovery sections; the Milestone 001 content in these files (and in `architecture/skill*.md`) remains the last update.
- **No new migration.** `STORAGE_LAYOUT_VERSION = 28` (`crates/codegg-core/src/storage/mod.rs:39`). The most recent schema migration, `migrate_v28` (`crates/codegg-core/src/session/schema.rs:1324`), is Project Catalog M1, not M2. No additive migration was identified because the resolver, scan-state store, and discovery-config schema are absent.

## 4. Verification executed

Per the planning process, this closure record reports what was actually run and observed rather than an aspirative verification log. The §11 commands that were run for this milestone are listed in §2.2 above; the commands that *can* be run cleanly against current `main` but were not separately re-executed for this milestone are listed below for the corrective plan's reference:

```bash
rtk cargo check --workspace --all-features
rtk cargo test -p codegg-providers
rtk cargo test -p codegg-protocol
rtk cargo test --test session_crud
rtk cargo test --test workspace_isolation
rtk cargo test --test workspace_services_isolation
rtk cargo test --test asset_snapshot
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk python3 scripts/check_project_agent_pwd_inference.py
```

These remain the verification standard for the eventual corrective pass; running them now would only confirm that prior milestones remain green and would not change the corrective-pass disposition.

### Static-guard status

- `scripts/check-core-boundary.sh`: pass (pre-existing; nothing in this milestone touched `codegg-core`).
- `scripts/check_daemon_cwd_usage.py`: pass (no violations).
- `scripts/check_identity_path_usage.py`: pass (no forbidden construction found).
- `scripts/check_project_catalog_invariants.py`: pass (7/7 checks; pre-existing M1 guard).
- `scripts/check_execution_ownership.py`, `scripts/check_scheduler_bypass.py`, `scripts/check_git_forbidden_patterns.py`, `scripts/check_project_agent_pwd_inference.py`: pass (per prior closure runs; not affected by this milestone).
- No `scripts/check_discovery_*.py` exists; the planned M2-specific guard is unfulfilled.

## 5. Invariant review

Each source-plan invariant (plan §4) is reviewed against the current `main`.

- **Discovery roots are explicit configuration, never process cwd or an implicit home-directory sweep.** No scanner exists; the no-sweep invariant is preserved by absence. The M1 `restart_hydration` (closed in `a2db5e4`) was already probe-free and remains so; `check_daemon_cwd_usage.py` continues to pass. **Preserved**.
- **Paths are observations/locators, not project identity.** No new code path constructs `ProjectId` from a path. `Locator::Local` continues to reference a registered `WorkspaceId` (`project_catalog.rs` M1); `scripts/check_identity_path_usage.py` continues to pass; M1 static guard `scripts/check_project_catalog_invariants.py:check_no_unwrap_default_pathbuf` continues to pass. **Preserved**.
- **Scanning is metadata-only and cannot activate LSP, indexers, build systems, agents, providers, or workspace service bundles.** No scanner exists. **Trivially preserved**; the affirmative guard that would extend M1's coverage into the scanner module is not in place.
- **Git inspection remains local-only, bounded, non-interactive, hook-free, and network-free.** No new Git probe code was authored. The pre-existing `egggit` substrate remains the only Git-inspection path and is unchanged. **Preserved**.
- **Durable catalog records are not deleted because a root or candidate is temporarily unavailable.** No absence-handling code exists. The M1 archive path (`ProjectCatalog::archive_project`) is operator-driven and not auto-invoked. **Trivially preserved**.
- **Reconciliation favors false negatives over merging unrelated repositories or forks.** No reconciliation engine exists. **Trivially preserved**.
- **Symlink aliases and canonical path aliases cannot create duplicate observations.** No observation table exists. **Trivially preserved**.
- **Remote SSH/linked-node locators remain inert and are not scanned by this milestone.** `Locator::Ssh` and `Locator::LinkedNode` remain inert per M1 closure §5; `scripts/check_project_catalog_invariants.py:check_no_unwrap_default_pathbuf` continues to enforce no SSH/LinkedNode path coercion. **Preserved**.
- **Scan work is bounded by depth, entries, elapsed time, concurrency, and diagnostics/output size.** No scanner exists. **Trivially preserved**.
- **Concurrent scans of the same configured root coalesce or serialize; they do not publish conflicting generations.** No scanner exists. **Trivially preserved**.

The first invariant (explicit-root-only) is the one the milestone was meant to enforce. It is preserved by absence because no scanner code was authored; the corrective plan must establish the affirmative static guard and the runtime bounded-config validation.

## 6. Failure and recovery review

This section is intentionally short. The plan's §8 failure semantics assume the existence of the scanner, scan-state store, coordinator service, and reconciliation engine. Because none of those exist:

- Invalid root configuration: no scanner can return `InvalidRoot`; the absent path means invalid configs are never surfaced. The corrective plan must establish typed validation outcomes.
- Missing/unavailable root: no scanner can record `Unavailable`; the M1 catalog absence-preservation property continues to hold by absence of automatic deletion logic.
- Permission failures: no scanner runs `read_dir`, so no permission diagnostics exist. The corrective plan must establish the bounded diagnostic shape.
- Truncation: no scanner exists; the corrective plan must establish the truncation reporting contract.
- Cancellation: no scanner exists; the corrective plan must establish the cancellation contract that publishes no completed generation.
- Single-flight: no scanner or coordinator exists; the corrective plan must establish the single-flight primitive and the typed already-running response.
- Overlapping roots: no scanner exists; the corrective plan must establish canonical candidate identity and reconciliation uniqueness.
- Reconciliation conflicts: no reconciliation engine exists; the corrective plan must establish expected-revision conflict semantics.
- Restart: no scanner exists; the M1 `restart_hydration` remains probe-free (already evidenced).

No new failure mode has been introduced. The corrective plan must re-examine these properties once the scanner, store, coordinator, and reconciliation engine are landed.

## 7. Migration and compatibility review

- v25 binding tables and v28 catalog tables are present (closed in prior milestones). The plan required an additive migration only when a concrete missing constraint/index/compatibility-state field was required; no such field was identified because the resolver and scanner are absent.
- Historical tables and fields are intact: the M1 catalog tables remain compatibility projections, and the M2 plan does not specify a shape change to any M1 table.
- `STORAGE_LAYOUT_VERSION = 28` (`crates/codegg-core/src/storage/mod.rs:39`). No schema bump is appropriate; no additive fields exist.
- The M1 `project_locator` table (Local/Ssh/LinkedNode inert placeholders) is untouched; the M2 plan does not call for a new locator kind.
- CLI commands: no command was added; the 107-command ledger holds.
- Provider registration, environment-variable auto-registration, and TUI behavior are unaffected.

## 8. Security review

The milestone's security posture is unchanged from Milestone 001 because no scanner code landed. None of the security-sensitive concerns the plan enumerates are introduced, regressed, or remedied:

- **Canonicalization of roots and candidates.** No scanner exists; the corrective plan must establish canonicalization before any path comparison.
- **Reject NUL/control/oversized paths and configuration fields.** The corrective plan's bounded config schema is unfulfilled.
- **Disable Git prompts, hooks, credential helpers, and network access.** The pre-existing `egggit` and `git_noninteractive` test (closed with Domain Identity 002 and re-asserted by `cargo test --test git_noninteractive` in this closure run) already enforce probe parameters for credentialed remote Git. The corrective plan's scanner must accept these flags rather than introduce a new code path.
- **Redact credential-bearing remotes and do not persist unsafe lineage evidence.** The Milestone 001 archive semantics guarantee that locator fields are bounded; the M2 scanner must extend the same bounds.
- **No arbitrary project file contents beyond bounded marker/stat/Git metadata.** No scanner exists; the affirmative guard is unfulfilled.
- **Symlink/hardlink/path-traversal escape.** The M1 `scripts/check_project_catalog_invariants.py` covers path-coercion but not traversal. The corrective plan must add a static guard (e.g., `scripts/check_discovery_invariants.py`) that rejects scanner code which constructs a `PathBuf` from an unverified root string, dereferences a non-canonical path, or follows symlinks outside the configured root.
- **Bounded diagnostic paths.** The corrective plan must establish the diagnostic shape.

No new untrusted-root or untrusted-candidate surface was introduced because no scanner was authored. The corrective plan inherits and must implement the security obligations above.

## 9. Documentation and operations

- `AGENTS.md` and `architecture/overview.md` ledger was not modified; 107-command count, 44-event count, 39 LSP servers, and ~37 tools remain accurate.
- No architecture document was updated. The plan §12 requires updates to `architecture/project_catalog.md` (M2 bounded-discovery section), `architecture/project_identity_storage.md` (scanner/candidate-evidence section), `architecture/storage.md` (any M2 additive migration), `architecture/workspace.md` (discovery-root policy), `architecture/workspace_services.md` (scanner isolation guarantees), and `architecture/config.md` (M2 config schema). None of these updates landed.
- No new operator command, CLI surface, or recovery procedure is required by this corrective pass. The M2 plan's §16 "Handoff notes" guidance (treat `3ce0a7e` as the reviewed production baseline; preserve existing catalog/project-storage write authority; prefer conservative under-discovery; do not turn this milestone into an always-on background crawler) carries forward unchanged into the corrective plan.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| critical | Milestone 002's principal capability boundary (discovery config, scanner, reconciliation engine, scan-state store, coordinator service, restart-safe retention) is not implemented. | Catalog cannot grow beyond explicit registration; operators cannot scope discovery to bounded source roots; reconciliation/identity-churn invariants cannot be exercised; Phase 3 long-term exit criteria for discovery and reconciliation remain unmet. | Corrective implementation pass under `plans/implementation/project-catalog/003-…` or this plan re-handed, referencing this closure record. |
| critical | No M2 static guard. `scripts/check_discovery_*.py` does not exist. The scanner cannot activate workspace services, write to candidate repositories, follow symlinks out of roots, or coerce path text into `ProjectId`/`WorkspaceId` without a guard to confirm it. | Future scanner code could regress the documented discovery invariants (no-activation, no-write, symlink-containment, path-independent identity) without CI catching the regression. | Implement `scripts/check_discovery_invariants.py` (or equivalent) alongside the corrective plan's scanner module. |
| high | Plan §11 verification command `cargo test --test project_catalog` is incorrect as written; the actual integration test target is at `crates/codegg-core/tests/project_catalog.rs`. | Operators running the prescribed verification commands verbatim will encounter an `error: no test target named 'project_catalog'` failure that has no bearing on M2 readiness. | Corrective plan §11 must be updated to `cargo test -p codegg-core --test project_catalog` and any other test target that has moved under a crate. |
| medium | M2 plan's §10 "Required tests" describes an integration test file `tests/project_catalog.rs` and integration files for bounded scanner, restart, contention, security, and scale; none of these exist. | The plan's test enumerations need alignment with the actual workspace test layout (`crates/codegg-core/tests/` for codegg-core integration tests; `tests/` for root crate integration tests). | Corrective plan must re-author its test list using the established crate convention. |
| low | Pre-existing fmt diff in `tests/skills_registry.rs` (a stray `use` import between test functions). | Formatting check fails at HEAD, but the diff is unrelated to M2 and was already present at the last closure. | Out of scope for this record; tracked for the runtime-assets subsystem next time it touches `tests/skills_registry.rs`. |
| low | Pre-existing pedantic-clippy warnings (11 in total, 0 errors without `-D warnings`). All are unrelated to discovery: `crates/codegg-core/tests/project_catalog.rs:255, 614, 621` (used `assert_eq!` with literal bool; redundant `clone`-to-`from_ref`); `src/tool/skill.rs:59` (`.into_iter()` in argument position); `tests/session_selection.rs:66` (very complex type); `tests/skills_registry.rs:165, 256` (borrowed expression / redundant clone); `src/skills/parser.rs:172` (redundant field names); `src/skills/registry.rs:244, 281` (`sort_by_key`, collapsible `if`); `src/tui/app/types.rs:84` (large size difference between variants). | None touch discovery code paths because no discovery code was authored; pedantic-clippy noise persists from the prior closures. | Tracked under each owning subsystem on its next touching commit; not duplicated here. |
| low | A previous Provider Connections 003 closure recorded a flaky `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog` test; the broad run in §2.2 did not encounter it on this run (7965 passed, 10 ignored, 0 failed). | Unrelated to M2. The M2 corrective pass should record the same flakiness note if it recurs. | Track and fix in the provider-connections subsystem. |

No new high-severity defect has been introduced because no new code has landed. The critical finding is structural: **the milestone's principal capability boundary is absent**; that absence is the corrective pass.

## 11. Roadmap disposition

**Corrective pass required** (per `plans/003-planning-process.md` §7 and `plans/closure/README.md`'s closure rules). A corrective implementation plan is required to perform the work described in the original plan:

- the bounded `DiscoveryConfig` schema with finite defaults;
- the `DiscoveryRoot`, `DiscoveryMode`, `DiscoveryCandidate`, `DiscoveryObservation`, `DiscoveryReport`, and scan-lifecycle types;
- the additive `migrate_vN` schema migration for discovery roots, scan runs, observations, and retention;
- the bounded local scanner with deterministic ordering, depth/entry/candidate/time/concurrency bounds, ignore/permission/symlink handling, Git/directory/mixed modes, and `repository_lineage`-backed detection that does not descend into `.git` internals and does not start another subsystem;
- the conservative reconciliation engine implementing the plan's §6 ordered evidence rules (existing local locator → canonical alias → unique lineage → explicit association → CodeGG-owned marker → new candidate → ambiguous/fork split);
- the scan-state store tracking generations, observations, and retention; comparing each completed scan with the prior generation; marking present/moved/missing/ambiguous/inaccessible/ignored/stale without deletion;
- the core-neutral coordinator service with list/get roots, validate, preview, refresh-one, refresh-all, cancel, scan status/report, and list unresolved/ambiguous observations;
- the single-flight per root and bounded global concurrency for refresh-all;
- the discovery-aware continuation of `conservative_legacy_association` whose ambiguity outcomes now flow from the candidate-evidence engine rather than the legacy `rebind_required` diagnostic;
- the static guard `scripts/check_discovery_invariants.py` (or equivalent) covering: scanner does not import/start LSP/indexer/provider/agent/build/workspace-service activation; remote locator fields never coerce to local paths; path text never constructs a `ProjectId`; scanner does not write under candidate roots; scans have explicit bounds;
- the §12 documentation updates for `architecture/project_catalog.md`, `architecture/project_identity_storage.md`, `architecture/storage.md`, `architecture/workspace.md`, `architecture/workspace_services.md`, and `architecture/config.md`;
- the corrected §11 verification commands reflecting the actual crate layout (`cargo test -p codegg-core --test project_catalog` rather than the root `cargo test --test project_catalog`).

The original plan `plans/implementation/project-catalog/002-bounded-discovery-reconciliation.md` remains the authoritative source of requirements for the corrective pass. The implementation agent re-handed the plan must, per §7 of the planning process:

- reference this closure record in the corrective plan or re-handoff notes;
- include regression tests and at least one new static-guard negative fixture that would have failed under the present state (e.g., a fixture that exercises scanner-side activation that should be rejected; a unit test that confirms reconciliation preserves the in-flight generation on cancellation; a fixture that exercises overlapping-root deduplication; a static-guard violation in a scanner module that calls `ProjectId::new(scan_root.as_os_str())` or `tokio::fs::create_dir_all(candidate_root)`);
- avoid reopening already-closed Milestones 001 (catalog foundation), Domain Identity 001/002 (identity foundation and repository storage migration), or Runtime Assets 001 (source-aware asset registry).

### Unblocking of downstream work

| Downstream plan | Hard / Interface / Soft? | If M2 closed successfully, would unblock? | If M2 corrective, restated blocker |
|---|---|---|---|
| Project Catalog Milestone 003 — Lazy Activation and Health (`plans/subsystems/project-catalog-roadmap.md#milestone-3--lazy-activation-and-health`) | Hard on M1–M2; interface on workspace-service registry | yes — sole remaining hard dependency is M2 (Runtime Assets M2 closed; Runtime Assets M3 has hard dep only on M1 and an interface dep on M2 already satisfied for snapshot-builder purposes) | M2 closure; the runtime-asset refresh interface dependency was already satisfied by Runtime Assets M2 (`e8934d1`), so the restated blocker is precisely "Project Catalog Milestone 002 closure (bounded discovery and reconciliation)." |
| Project Catalog Milestone 004 — Protocol/Server Migration (`plans/subsystems/project-catalog-roadmap.md#milestone-4--protocol-server-migration-and-closure`) | Hard on M1–M3; soft/SHOULD on runtime-asset activation refresh | no — depends on M3, which depends on M2 | M2 → M3 closure; M2 closure transitively unblocks M4 only after M3 lands. |
| Multi-Project TUI Milestone 001 (`plans/implementation/tui-project-sessions/001-project-aware-state.md`) | Hard on Project Catalog **M4** and Runtime Assets **M3** (not M2 directly) | no | M4 + Runtime Assets M3 (no change from current blocker). |
| Session Projections Milestone 001 (`plans/implementation/session-projections/001-projection-contracts.md`) | Hard on Domain Identity M3, Project Catalog **M4**, Multi-Project TUI M1 (not M2 directly) | no | DI-3 corrective + M4 + TUI-1 (no change from current blocker). |
| Runtime Assets Milestone 003 (`plans/subsystems/runtime-assets-roadmap.md#milestone-3--refresh-lifecycle-and-operator-surface`) | Hard on Runtime Assets M1; interface on Runtime Assets M2; **no dependency on Project Catalog M2** | no | None from this milestone. However, the runtime-assets roadmap §12 status row's blocker text reads "Milestone 2 closure" which is **ambiguous between Runtime Assets M2 and Project Catalog M2**; the corrective closure for Project Catalog M002 coincides with a clarification that the Runtime Assets M3 blocker is "Runtime Assets Milestone 2 closure / snapshot-builder interface," not "Project Catalog Milestone 2 closure." This record recommends the runtime-assets-subsystem roadmap's §12 row be disambiguated by the next runtime-assets closure. |

**Headline unblocking statement:** No downstream plan that names Project Catalog M2 as a hard dependency is unblocked by this closure. The closest consumer (Project Catalog M003 — Lazy Activation and Health) remains blocked on M002 closure. All other downstream plans name M4 (not M2) and are transitively blocked through M3.

**Disambiguating cross-subsystem references:**

- `plans/subsystems/runtime-assets-roadmap.md` §12 status row for Milestone 3 reads "Milestone 2 closure (snapshot-builder interface and ProjectAssetSnapshot available)" — this refers to **Runtime Assets M2**, not Project Catalog M2. The string is ambiguous; future readers should consult the §12 closure link column, which points at `plans/closure/runtime-assets/002-status.md`.
- `plans/closure/provider-connections/004-status.md` §11 (Roadmap disposition) lists "Milestone 004" as a dependency for a refresh/activation contract. That "Milestone 004" is a Provider-Connections-internal reference scoped to its own roadmap M4 (corrective lifecycle/rotation). **It is not Project Catalog M4.** That closure's disposition is unaffected by this record.
- `plans/implementation/domain-identity/003-daemon-protocol-adoption.md` cites Project Catalog 4 as the owner of the catalog protocol/server migration; that is **Project Catalog M4**, not M2, and is unchanged by this record.

### Future plan registration

A corrective implementation plan should be created at `plans/implementation/project-catalog/003-bounded-discovery-corrective.md` (or this same plan re-handed with new evidence requirements and the corrected §11 verification commands). Its baseline should be `4e974f0` (current `main`), its class should remain **capability** (the plan's primary class is unchanged), and its dependencies are hard on the existing Milestones 001 (project catalog foundation, `a2db5e4`) and Domain Identity 002 (repository storage, `84d92f0`).

## 12. Registry updates

- `plans/registry.md`:
  - Move Project Catalog 002 from the "Dependency-ready implementation plans" table (line 40) to the "Active closure work" table with status `corrective pass required`, linking this closure record.
  - Do **not** register a corrective plan in the "Dependency-ready implementation plans" table until it is authored.
  - Keep the "Blocked work" rows for Multi-Project TUI M1 and Session Projections M1 unchanged; this record confirms that the Project Catalog M4 portion of their conjunctive blockers is unchanged.
  - Keep the "Recently closed work" rows for Project Catalog M001, Domain Identity M001/M002, Runtime Assets M001/M002, and Provider Connections M001/M002/M003/M004 unchanged.
- `plans/subsystems/project-catalog-roadmap.md` §12:
  - Update the Milestone 2 status row from `ready` to `corrective pass required`, link this closure record, and keep Milestones 3 and 4 in their current `not started` state with their blocker text now reading "Project Catalog M002 closure" rather than "Milestones 1–2 closure" or "Milestones 1–3 closure."
  - Do **not** modify any canonical long-term document.
- Do **not** archive the implementation plan; do **not** mark Milestone 002 closed in any table.

## 13. Handoff notes

- The plan's §16 "Handoff notes" continue to apply to the corrective plan verbatim:
  - The baseline is `4e974f0` (current `main`); treat the original `3ce0a7e` as the historical reviewed state.
  - Preserve the existing catalog/project-storage write authority; do not introduce a parallel write path.
  - Runtime Assets M002 (`e8934d1`) and Domain Identity M003 (corrective-pass-required, closure record `plans/closure/domain-identity/003-status.md`) may proceed in parallel; use their stable closed interfaces only unless their new APIs are already merged.
  - Follow the repository's resource-conscious test configuration (`CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14` and narrower crate-scoped runs).
  - Prefer conservative under-discovery over false project merges.
  - Do not turn this milestone into an always-on background crawler; explicit bounded refresh is the correctness surface.
- The corrective plan must update §11 verification commands to reflect the actual crate layout (`cargo test -p codegg-core --test project_catalog` instead of `cargo test --test project_catalog`) and to clarify that broad-suite runs use capped build jobs and test threads.
- The corrective plan must include a positive static-guard fixture and at least one negative fixture exercising scanner-side activation that would fail the new guard.
- A repeated corrective pass without progress would indicate that the project-catalog roadmap's milestone sizing should be revised; the corrective plan is sized to one implementation agent pass per `plans/003-planning-process.md` §5.
