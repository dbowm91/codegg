# Project Catalog Milestone 002 — Bounded Discovery and Reconciliation

Status: closed — see `plans/closure/project-catalog/002-status.md`

Repository baseline: `3ce0a7ea7c1a8baa41a2618eb293291435e9f9f0` (`main`; planning-only commits after this baseline do not alter production behavior)

Production implementation baseline:

- `5974976` — bounded discovery configuration, scanner, reconciliation coordinator, schema v29 persistence, guards, tests, and architecture documentation.

- `84d92f0` — canonical project/repository/workspace/session storage and lineage reconciliation.
- `a2db5e4` — durable project catalog foundation, inert locators, archive/restore, health placeholders, and probe-free restart hydration.
- `f9db5c3` — bounded source-root and path-safety patterns in the runtime asset registry.

Source roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-2--bounded-discovery-and-reconciliation`

Long-term requirements:

- `plans/000-long-term-specification.md` — durable project catalog, explicit discovery roots, lazy activation, and path-independent identity.
- `plans/001-terminology-and-domain-model.md` — Project, ProjectLocator, Repository, Workspace, discovery candidate, observation, reconciliation, and health.
- `plans/002-long-term-roadmap.md#phase-3--project-catalog-discovery-and-lazy-activation`

Applicable closure evidence:

- `plans/closure/domain-identity/002-status.md`
- `plans/closure/project-catalog/001-status.md`
- `plans/closure/runtime-assets/001-status.md`

Applicable ADRs:

- None. The canonical documents already choose conservative reconciliation and path-independent identity. Stop for an ADR if implementation requires treating several repositories as first-class co-equal project identity or introducing semantic monorepo subproject inference.

Primary class: capability

## 1. Objective

Discover candidate local projects beneath explicitly configured roots using bounded, cancellable, metadata-only scans; reconcile candidates conservatively into the durable project catalog; and update locators/observations without activating workspace services or creating identity churn.

The milestone succeeds when large directory forests can be scanned within deterministic limits, known Git lineage and canonical workspace aliases converge safely, path moves update locators when evidence is sufficient, ambiguous candidates remain separate or require explicit association, and temporary root unavailability never deletes durable catalog records.

This milestone does not activate projects, start LSP/index/build services, expose the complete project protocol, or scan remote SSH/linked-node locators.

## 2. Why this milestone is ready

The hard dependency is closed:

- Project Catalog Milestone 001 provides stable project/repository records, local/SSH/linked-node locator records, lifecycle, archive/restore, health placeholders, explicit local registration, and restart hydration.

The reconciliation substrate is also closed:

- Domain Identity Milestone 002 provides bounded local Git lineage inspection, uniqueness constraints, conservative ambiguity handling, and canonical workspace/project/repository binding stores.
- Runtime Assets Milestone 001 provides established patterns for explicit roots, source classification, symlink containment, size/count limits, diagnostics, and read-only discovery.

## 3. Current implementation evidence

At the repository baseline:

- `codegg_core::project_catalog::ProjectCatalog` exposes list/get/register/archive/restore, locator attach/detach, health records, workspace/session listing, lifecycle counts, and probe-free restart hydration.
- `Locator::Local` references an already registered workspace; SSH and linked-node locators are inert data.
- `ProjectStorage` can reconcile an explicit workspace path with bounded local Git lineage and can preserve ambiguous evidence as `rebind_required` diagnostics.
- `conservative_legacy_association` is idempotent and operates only on already registered workspaces.
- No configured-root scanner, scan-state store, discovery generation, candidate record, incremental reconciliation report, or daemon discovery coordinator exists.
- Catalog listing and restart hydration intentionally perform no filesystem or Git probing.
- Remote locators serialize but cannot be scanned or executed locally.
- Project Catalog Milestone 001 records a conservative no-`.git` workspace as requiring explicit rebind rather than guessing.

## 4. Invariants that must not regress

- Discovery roots are explicit configuration, never process cwd or an implicit home-directory sweep.
- Paths are observations/locators, not project identity.
- Scanning is metadata-only and cannot activate LSP, indexers, build systems, agents, providers, or workspace service bundles.
- Git inspection remains local-only, bounded, non-interactive, hook-free, and network-free.
- Durable catalog records are not deleted because a root or candidate is temporarily unavailable.
- Reconciliation favors false negatives over merging unrelated repositories or forks.
- Symlink aliases and canonical path aliases cannot create duplicate observations.
- Remote SSH/linked-node locators remain inert and are not scanned by this milestone.
- Scan work is bounded by depth, entries, elapsed time, concurrency, and diagnostics/output size.
- Concurrent scans of the same configured root coalesce or serialize; they do not publish conflicting generations.

## 5. Scope

### In scope

- Project discovery configuration schema.
- Explicit discovery-root records with stable IDs/revisions.
- Local filesystem scanner with Git, directory, or mixed modes.
- Depth, entry, duration, stat-concurrency, Git-probe-concurrency, ignore, permission, and symlink bounds.
- Metadata-only candidate records and diagnostics.
- Conservative candidate reconciliation into `ProjectCatalog`/`ProjectStorage`.
- Incremental scan generations and observation status.
- Dry-run/preview and apply reports through core-neutral service APIs.
- Root-unavailable and stale-observation behavior.
- Cancellation, coalescing, restart, contention, scale, and security tests.
- Operator-facing diagnostics through existing CLI/core service seams where possible, without implementing the multi-project TUI.

### Explicitly out of scope

- Remote SSH or linked-node scanning.
- Project/workspace service activation, leases, idle eviction, LSP, indexing, or builds.
- Semantic monorepo package discovery.
- Automatically treating every nested directory as a project.
- Writing markers or configuration into discovered repositories.
- Following symlinks outside configured roots.
- Full project catalog protocol/REST/WS migration; Milestone 004 owns that surface.
- Multi-project TUI tabs/picker.
- Deleting projects or workspaces when candidates disappear.
- Team authorization.

## 6. Required production changes

### Discovery configuration

Add a bounded configuration model under the existing config system, with a structure equivalent to:

- stable root name/ID;
- local root path;
- mode: `git`, `directory`, or `mixed`;
- enabled flag;
- maximum depth;
- maximum visited entries;
- maximum candidates;
- maximum elapsed time;
- stat/read concurrency;
- Git probe concurrency;
- include-hidden policy;
- symlink policy defaulting to no-follow;
- ignore names/patterns;
- optional explicit directory markers or direct-child-only policy for non-Git mode.

Requirements:

- safe defaults must be conservative and bounded;
- roots must canonicalize and remain inside configured policy boundaries;
- duplicate/overlapping roots must be diagnosed and scanned deterministically;
- unsupported or oversized patterns fail configuration validation;
- configuration reload changes future scans but does not silently remove catalog records.

### Discovery domain and persistence

Introduce domain types for:

- `DiscoveryRoot` / stable root identity and revision;
- `DiscoveryMode`;
- `DiscoveryCandidate` containing canonical locator, optional detected repository facts, evidence strength, candidate kind, source root, relative depth, and diagnostics;
- `DiscoveryObservation` linking a scan generation to a candidate/project/workspace decision;
- `DiscoveryReport` with visited/ignored/candidate/reconciled/ambiguous/unavailable/error counts, duration, truncation flags, and bounded diagnostics;
- reconciliation outcome/reason enums;
- scan lifecycle: queued/running/completed/cancelled/failed/truncated.

Persist only the metadata needed for restart inspection, incremental comparison, and stable observations. An additive schema may include discovery roots, scan runs, and observations. Do not persist unbounded filesystem details or file contents.

Every table/field must have explicit bounds, indexes for root/generation/project/workspace/status, and retention policy for old scan runs. Retention may prune old observations but must not delete project/catalog authority.

### Scanner

Implement a local scanner that:

1. starts from an explicitly configured canonical root;
2. traverses with deterministic ordering;
3. enforces maximum depth, entries, candidates, elapsed time, and concurrency;
4. skips default heavy/irrelevant directories such as `.git` internals, build outputs, caches, dependency/vendor trees, and CodeGG runtime state according to documented safe defaults;
5. does not follow symlinks by default and rejects escape when allowed aliases are inspected;
6. handles permission errors as bounded diagnostics rather than aborting the whole root where safe;
7. identifies Git worktrees/repositories without descending through their `.git` metadata;
8. uses `repository_lineage` only after a candidate passes path/policy checks;
9. supports a conservative directory mode only through explicit policy, direct-child rules, known project markers, or existing catalog/workspace evidence;
10. never writes to candidates and never starts another subsystem.

A Git repository root should normally terminate descent unless configuration explicitly permits nested repository detection within a bounded remaining depth.

### Reconciliation

Implement a deterministic reconciliation policy in this order or an equivalent documented order:

1. exact existing local locator/workspace match;
2. canonical-path alias of an existing workspace;
3. unique canonical repository lineage match;
4. explicit operator/config association;
5. existing stable CodeGG-owned catalog/discovery marker if one already exists and is verified without writing;
6. otherwise create a new candidate/project only when policy allows and evidence is sufficient;
7. ambiguous/fork-like/conflicting evidence remains unassociated or creates a distinct project with diagnostic, never merged silently.

Required behavior:

- a unique lineage match may attach/update a local locator for an existing logical project and repository;
- a workspace move/rename updates locator/observation while preserving project identity when lineage remains unique;
- repositories sharing a remote but having conflicting local lineage/fork evidence must not merge merely by URL;
- plain directories without durable evidence must not be treated as the same project after a move unless explicit association exists;
- discovery may register a workspace/project through the existing canonical service APIs, not direct duplicate SQL;
- reconciliation writes are transactional and revision-safe;
- preview/dry-run and apply use the same decision engine.

### Incremental refresh and stale observations

- Track scan generation/revision per discovery root.
- Compare new candidates with the prior completed generation using canonical locator plus repository facts.
- Mark observations as present, moved, missing, ambiguous, inaccessible, ignored, or stale.
- A missing candidate does not archive/delete the project automatically.
- A temporarily unavailable root leaves its last successful generation intact and records root health/diagnostics.
- Successful scans may update catalog health/observation timestamps through bounded metadata only.
- Restart uses durable catalog plus last completed scan metadata; it does not force an immediate scan.

### Coordinator/service seam

Add a core-neutral service exposing operations equivalent to:

- list/get discovery roots;
- validate configured roots;
- preview/dry-run one root;
- refresh one root;
- refresh all enabled roots with bounded global concurrency;
- cancel a scan;
- get scan status/report;
- list unresolved/ambiguous observations;
- explicitly associate a candidate with an existing project/workspace when supported by current operator APIs.

The service must support single-flight per root and bounded global scan concurrency. It should be usable later by daemon protocol handlers without importing Axum or TUI types into `codegg-core`.

### Security

- Canonicalize roots and candidates before comparison.
- Reject NUL/control/oversized paths and configuration fields.
- Disable Git prompts, hooks, credential helpers where possible, and network access.
- Redact credential-bearing remotes and do not persist unsafe lineage evidence.
- Never read arbitrary project file contents beyond bounded marker/stat/Git metadata required by the configured mode.
- Prevent symlink/hardlink/path traversal escape.
- Bound diagnostic paths and do not emit entire directory listings.

### Documentation and static guards

Update at least:

- `architecture/project_catalog.md`;
- `architecture/project_identity_storage.md`;
- `architecture/config.md`;
- `architecture/storage.md`;
- `architecture/workspace.md`;
- `architecture/workspace_services.md`.

Add static checks or focused review guards proving:

- scanner code does not import/start LSP, indexer, provider, agent, build, or workspace service activation;
- remote locator fields are never converted into local paths;
- path text never constructs a `ProjectId`;
- discovery does not write under candidate roots;
- scans have explicit bounds.

## 7. Ordered work packages

### Work package A — Configuration and discovery domain

Intent: define bounded inputs and inspectable outputs before filesystem traversal.

Required changes:

- add config schema/defaults/validation;
- define root/candidate/observation/report/outcome types;
- define scan lifecycle and truncation semantics;
- add additive persistence when required.

Acceptance evidence:

- invalid/overlapping/unbounded roots produce actionable diagnostics;
- defaults are finite and conservative;
- schema migration is idempotent and restart-safe.

### Work package B — Bounded local scanner

Intent: enumerate candidates without activation or unbounded work.

Required changes:

- deterministic traversal;
- depth/entry/candidate/time/concurrency bounds;
- ignore/permission/symlink handling;
- Git/directory/mixed detection;
- local bounded repository fact extraction.

Acceptance evidence:

- large fixture truncates predictably within limits;
- permission and symlink failures do not escape or crash the scan;
- no service activation side effects occur.

### Work package C — Conservative reconciliation engine

Intent: turn observations into stable catalog updates without false merges.

Required changes:

- implement ordered evidence rules;
- reuse existing catalog/project-storage APIs;
- support preview and apply through one decision engine;
- add typed ambiguity/conflict outcomes;
- update locators for verified moves.

Acceptance evidence:

- aliases and same unique lineage converge;
- forks/ambiguous remote matches remain separate;
- path move preserves identity only with sufficient evidence;
- plain-directory uncertainty remains explicit.

### Work package D — Incremental scan state and recovery

Intent: make discovery repeatable and restart-safe.

Required changes:

- persist generations/runs/observations;
- compare with prior completed generation;
- mark stale/missing/unavailable without deletion;
- retain last successful state across failed/cancelled scans;
- implement bounded retention.

Acceptance evidence:

- daemon restart can inspect last scan without rescanning;
- cancelled/failed scan does not replace last completed generation;
- unavailable root preserves catalog state.

### Work package E — Coordinator and operator seams

Intent: make discovery controllable without building the TUI/protocol yet.

Required changes:

- single-flight per root;
- bounded global concurrency;
- preview/refresh/cancel/status/report APIs;
- structured diagnostics and explicit association seam.

Acceptance evidence:

- concurrent refresh requests coalesce;
- cancellation is prompt at traversal/probe boundaries;
- reports are bounded and deterministic.

### Work package F — Guards, docs, and scale closure

Intent: prevent discovery from becoming activation or an unbounded crawler.

Required changes:

- static guards/negative fixtures;
- architecture/config/storage documentation;
- performance and scale fixtures;
- record deferred remote/activation/protocol work.

Acceptance evidence:

- guards fail deliberate activation/write/path-identity violations;
- documented bounds match defaults and tests.

## 8. Failure, cancellation, restart, and contention semantics

- Invalid discovery-root configuration prevents that root from scanning and returns diagnostics; unrelated valid roots remain usable.
- Missing/unavailable root records a failed/unavailable scan and preserves the prior completed generation and catalog.
- Permission failures inside a root are candidate/path diagnostics unless they prevent the root itself from being inspected.
- Reaching depth, entry, candidate, time, or output bounds returns a successful truncated report, not an unbounded continuation.
- Cancellation stops traversal and pending Git probes, publishes no completed generation, and leaves the prior completed generation authoritative.
- Concurrent scans for one root single-flight; callers receive the same operation/report or a typed already-running response.
- Different roots run under bounded global concurrency.
- Overlapping roots may discover the same candidate; canonical candidate identity and reconciliation uniqueness must converge without duplicate project creation.
- Reconciliation conflicts use expected revisions/uniqueness and return typed current state.
- Restart marks an in-progress operation interrupted, preserves its partial rows only as non-authoritative diagnostics if useful, and retains the prior completed generation.

## 9. Compatibility and migration

- Existing explicit project registration remains functional and authoritative.
- Existing catalog/project-storage APIs remain the only write authority; discovery is an additional caller.
- Existing project/workspace/session records are not rewritten or deleted merely because discovery is enabled.
- Discovery is disabled or empty by default unless explicit roots are configured, unless the current product configuration already specifies an accepted default root policy. Do not silently scan the entire home directory.
- Existing remote locators remain inert and unaffected.
- Any schema migration is additive and does not modify historical compatibility fields.
- Full protocol/TUI exposure is deferred to Project Catalog Milestone 004.

## 10. Required tests

### Focused unit tests

- configuration defaults, bounds, validation, overlapping-root diagnostics.
- deterministic traversal ordering and ignore policy.
- candidate classification for Git/directory/mixed modes.
- evidence-strength and reconciliation decision tables.
- report truncation and bounded diagnostics.

### Integration tests

- temporary forest with Git repos, nested repos, plain directories, hidden/ignored trees, symlink aliases, and permission failures.
- same unique repository lineage under two paths converges.
- fork/ambiguous lineage remains separate.
- workspace rename/move updates locator without identity churn when evidence is sufficient.
- plain directory move remains unresolved without explicit association.
- explicit registration and discovery converge on one project.
- scan starts no workspace services/LSP/indexer/build/provider work.

### Restart and recovery tests

- last successful generation survives restart.
- interrupted scan is marked interrupted and does not replace completed state.
- unavailable root preserves prior observations and catalog.
- additive migration reruns after injected partial failure.

### Contention and cancellation tests

- concurrent same-root refresh coalesces.
- overlapping roots do not duplicate project creation.
- cancellation stops traversal/probes and publishes no completed generation.
- bounded global concurrency across many roots.

### Security and negative tests

- symlink escape and path traversal rejected.
- credential-bearing remote evidence redacted/not persisted.
- Git probe performs no network, prompts, or hooks.
- remote locator cannot become local scanner root without explicit supported conversion.
- scanner performs no writes under candidate roots.
- oversized config/path/diagnostic values fail safely.

### Scale and performance tests

- entry/depth/time/candidate limits under a large synthetic tree.
- Git probe concurrency cap.
- bounded memory/report size.
- catalog list remains probe-free before and after discovery data exists.

## 11. Required verification commands

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-core project_storage
rtk cargo test -p codegg-core repository_lineage
rtk cargo test -p codegg-core project_catalog
rtk cargo test -p codegg-core workspace
rtk cargo test -p codegg-core
rtk cargo test --test project_catalog
rtk cargo test --test storage_migrations
rtk cargo test -p codegg-git
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk git diff --check
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14
```

Report known pre-existing flaky provider/scheduler tests accurately if they recur. Do not treat an unrelated timing failure as discovery evidence, and do not claim a fully green broad suite when it is not green.

## 12. Documentation updates

- Document root configuration, safe defaults, and scan modes.
- Document candidate evidence and reconciliation order.
- Explain why missing candidates do not delete projects.
- Explain Git/fork ambiguity and explicit association.
- Document cancellation, coalescing, generation, retention, and restart semantics.
- Document the strict boundary between discovery and activation.
- Document remote scanning as deferred.

## 13. Acceptance criteria

- Only explicitly configured local roots are scanned.
- Scans enforce finite depth, entry, candidate, duration, concurrency, and output bounds.
- Scanning performs no activation and no writes inside candidate repositories.
- Canonical aliases and unique lineage converge without duplicate projects.
- Ambiguous/fork-like evidence never merges silently.
- Verified path moves update locators without changing logical project identity.
- Temporary root/candidate absence does not delete or archive catalog records.
- Failed/cancelled scans preserve the last completed generation.
- Concurrent scans coalesce and overlapping roots converge transactionally.
- Catalog listing/restart hydration remain probe-free.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- reconciliation requires using a path or remote URL as canonical project identity;
- several repositories must become co-equal identity owners without an ADR;
- semantic monorepo package discovery is required;
- implementation would scan the home directory or unconfigured roots implicitly;
- remote SSH/linked-node scanning or execution becomes necessary;
- discovery requires starting LSP/indexer/build/provider/workspace services;
- a candidate repository would need to be modified to establish identity;
- the work expands into full project protocol/TUI or lazy activation.

## 15. Closure evidence required

The closure record must contain:

- exact implementation commit(s);
- configuration/default/bounds table;
- scanner and reconciliation architecture;
- requirement-to-evidence matrix;
- large-tree/truncation results;
- same-lineage, alias, move, fork, and plain-directory evidence;
- cancellation/coalescing/restart results;
- proof that scanning starts no services and writes no candidate files;
- schema/retention evidence when persistence changes;
- static-guard output;
- full verification command log and known unrelated failures;
- explicit interface handed to Project Catalog Milestone 003.

## 16. Handoff notes

- Treat `3ce0a7e` as the reviewed production baseline; inspect current `main` before editing.
- Preserve the existing catalog/project-storage write authority rather than duplicating SQL ownership.
- Runtime Assets 002 and Domain Identity 003 may land in parallel; use their stable closed interfaces only unless their new APIs are already merged.
- Follow the repository's resource-conscious test configuration.
- Prefer conservative under-discovery over false project merges.
- Do not turn this milestone into an always-on background crawler; explicit bounded refresh is the correctness surface.
