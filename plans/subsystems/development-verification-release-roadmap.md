# Development Verification and Release Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#3-non-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related ADRs:

- None. This roadmap codifies repository verification and release operations. It does not change CodeGG runtime ownership, protocol, storage, or product architecture. A future proposal to make GitHub Actions or another external service authoritative for releases would require a separate architecture decision.

## 1. Purpose and ownership boundary

This workstream owns the repository-maintained development verification and release apparatus:

- GitHub Actions workflows used to validate pull requests and mainline commits;
- canonical local verification commands and their resource limits;
- documentation describing quick, full, and change-specific verification;
- maintainer-operated crates.io release procedure;
- optional compatibility, audit, example, cross-target, and real-server checks;
- cleanup of scripts, profiles, artifacts, and evidence machinery that exist only to support overbuilt CI.

The subsystem consumes production tests, static guards, Cargo package metadata, examples, and compatibility harnesses owned by their respective runtime subsystems. It does not become the owner of those product contracts.

This workstream must not own:

- product release cadence or version selection;
- crates.io credentials;
- GitHub repository administration outside workflow files and documented release commands;
- CodeGG's runtime test-runner feature under `src/test_runner/`;
- scheduler-owned build and test execution inside CodeGG;
- the future external CI runner adapter described by long-term roadmap Phase 18;
- comprehensive proof artifacts for every ordinary development commit.

The governing rule is:

> Routine hosted CI provides fast, bounded regression feedback. Broader verification and publication remain explicit maintainer actions performed locally or through an intentionally invoked manual process.

## 2. Work classification

### Invariants

- Routine CI must remain bounded in runner count, build parallelism, test parallelism, network dependencies, and artifact production.
- GitHub Actions must not publish crates, create releases, choose versions, or determine release cadence.
- A failing verification command must remain actionable and must not be hidden behind evidence aggregation or duplicated reruns.
- Local verification must preserve the repository's known resource constraint: heavyweight workspace tests cannot fan out into uncontrolled compiler jobs, test threads, subprocesses, or language servers.
- Product correctness tests and static ownership guards must remain available even when removed from the routine hosted-CI path.
- Real external servers, cross-target builds, SDK examples, dependency audits, and release dry-runs must be opt-in unless a narrowly justified change requires them.
- The planning and documentation state must describe the commands actually used; `--test-threads=14` must not be called serial.

### Capabilities

- Contributors receive one concise hosted-CI result for a pull request or mainline commit.
- Developers can run a documented quick verification path during ordinary iteration.
- Maintainers can run a documented bounded full verification path before substantial merges or releases.
- Maintainers can publish intended crates to crates.io through a manual, reproducible checklist.
- LSP compatibility, plugin examples, cross-target builds, and audits remain runnable on demand without burdening every commit.

### Infrastructure

- A reduced `.github/workflows/ci.yml` with one ordinary verification lane.
- A small canonical local verification entry point or exact command contract.
- Explicit Cargo build/test resource limits.
- Package-publication inventory and dry-run commands.
- Optional subsystem-specific verification commands retained in documentation.
- Static checks preventing accidental reintroduction of automated publishing where proportionate.

### Polish

- Removal of duplicated jobs, stale workflow comments, redundant Nextest profiles, obsolete aggregation scripts, and misleading test terminology.
- Consolidated developer and maintainer documentation.
- Clear diagnostics distinguishing routine CI, local full verification, and release-only checks.
- A compact final maintenance surface that can be understood without reconstructing historical closure plans.

## 3. Non-goals

- Reducing production test coverage merely to make a dashboard green.
- Rewriting the test suite or runtime test-runner subsystem wholesale.
- Introducing a new build orchestrator, task runner, release framework, or bespoke YAML generator.
- Replacing Cargo with Nextest or making Nextest mandatory for contributors.
- Automatically publishing crates or GitHub releases from tags, pushes, pull requests, schedules, or workflow dispatch.
- Producing signed supply-chain attestations, SBOMs, provenance bundles, or platform installers in this workstream.
- Guaranteeing every supported target builds on every commit.
- Making all workspace crates public. Publication scope must be explicit and package-specific.
- Solving future external-CI backend integration for CodeGG jobs.
- Treating optional evidence artifacts as a prerequisite for ordinary development.

## 4. Current state

At baseline `39d0720f9748cabc978ad9b0a3a32c31c6bc84d1`, routine CI is spread across multiple independent jobs for generated assets, formatting, checking, Clippy, workspace tests, repeated plugin-focused tests, examples, dependency audit, and a four-target release-mode build matrix with uploaded artifacts.

The broad workspace test lane already includes the plugin tests that are later rerun in the plugin-focused lane. The testing architecture explicitly calls this duplication intentional for diagnostics and defense in depth, but the duplicated work materially increases turnaround time without adding unique contract coverage. The core-boundary static check is the unique requirement and can execute without rerunning plugin tests.

The routine workflow also builds release binaries for Linux and macOS targets and uploads them for every qualifying push or pull request. These artifacts do not answer the routine regression question and duplicate work later performed by the release workflow.

The tag-triggered release workflow builds five target binaries, stages artifacts, creates checksums, and automatically creates a GitHub Release. It does not publish to crates.io and conflicts with the maintainer decision that release cadence and publication remain manual.

The real-server LSP workflow runs five separately provisioned language-server jobs, uploads per-server reports, downloads them into an aggregation job, validates an evidence manifest, and retains the final artifact. It triggers weekly and on relevant mainline changes in addition to manual dispatch. The compatibility tests themselves are valuable; the scheduled and artifact-producing evidence system is not required for ordinary repository correctness.

The testing documentation contains a useful resource taxonomy, but it also maintains several overlapping concepts:

- Cargo test commands and resource classes;
- four Nextest profiles;
- timing-capture and baseline-comparison scripts;
- a broad all-features CI baseline;
- duplicated focused plugin execution;
- separate real-server evidence production;
- claims that a fourteen-thread test run is serial.

The root package metadata also assumes downloadable GitHub release assets through `package.metadata.binstall`, while the intended distribution path is manual crates.io publication. Publication scope and metadata therefore need an explicit maintainer-owned contract.

## 5. Target architecture

The target repository has one routine GitHub Actions workflow with one ordinary verification job. It runs for pull requests and mainline pushes, uses bounded Cargo resources, does not upload build artifacts, and does not execute release, audit, real-server, example-matrix, or cross-target work.

The local verification contract has two canonical tiers:

```text
quick
  formatting/static source checks
  default-feature workspace compile
  narrow deterministic tests suitable for iteration

full
  all required static ownership guards
  strict linting
  bounded broad workspace tests
  feature-specific checks selected by documented change triggers
```

The exact implementation may be a short shell script or documented Cargo commands, but there must be one authoritative definition rather than divergent commands in CI, `AGENTS.md`, `CONTRIBUTING.md`, and `architecture/testing.md`.

Change-specific verification remains explicit:

```text
server or WebSocket changes     -> server feature checks and transport tests
plugin changes                  -> plugin feature tests and example SDK/WASM checks
LSP protocol changes            -> fake-server integration and optional real-server smoke
release preparation             -> full verification, package dry-runs, optional audit/cross-build
platform-specific changes       -> the affected target or host build
```

Release ownership is entirely manual:

```text
maintainer chooses version and cadence
  -> verifies clean mainline state
  -> updates package versions/changelog as applicable
  -> runs bounded full verification
  -> runs cargo package/publish dry-runs
  -> publishes intended crates manually in dependency order
  -> tags and optionally creates a GitHub Release manually
```

No GitHub Actions workflow publishes, tags, creates a GitHub Release, or stores registry credentials.

Real-server compatibility and other expensive evidence remain executable through local commands. A manual workflow may be retained only if it is materially simpler than local setup and contains no schedule, push trigger, artifact aggregation, or release authority. The preferred endpoint is local documentation unless a concrete remote-only need is demonstrated during implementation.

## 6. Dependency graph

```text
Milestone 001 — Routine CI contraction
    |
    v
Milestone 002 — Canonical local verification contract
    |
    v
Milestone 003 — Manual crates.io release ownership
    |
    v
Milestone 004 — Optional integration evidence cleanup and closure
```

Dependency classes:

- Milestone 001 has no hard dependency beyond the current workflow and test baseline.
- Milestone 002 has a hard dependency on Milestone 001's accepted routine-CI boundary so local and hosted commands do not diverge.
- Milestone 003 has a hard dependency on Milestone 002's full-verification contract and an interface dependency on the workspace package graph.
- Milestone 004 has a hard dependency on Milestones 001–003 because it reconciles the remaining workflows, scripts, profiles, and documentation against the accepted end state.
- Product subsystem tests are interface dependencies throughout; this roadmap may change how they are invoked but not silently weaken their contracts.

## 7. Milestones

### Milestone 001 — Routine CI contraction

Class: infrastructure

Objective:

Replace the multi-lane routine workflow with one bounded verification job that provides fast, non-duplicated feedback and performs no release or artifact work.

Dependencies:

- Current mainline workflow and test baseline.
- No unresolved runtime architecture decision.

Deliverable boundary:

- Simplify `.github/workflows/ci.yml`.
- Remove routine cross-target release builds and artifact uploads.
- Remove duplicated plugin reruns, example matrices, and dependency auditing from the hot path.
- Preserve essential generated-asset and ownership guards in the single verification lane.
- Eliminate duplicate push/pull-request execution where possible.
- Record the exact bounded routine commands.

User or operator value:

- Faster iteration and fewer unrelated CI failures.
- One actionable result instead of a dependency chain of overlapping jobs.
- Lower runner and cache churn.

Exit conditions:

- One ordinary verification job is the only required pull-request gate.
- Routine CI creates no binary, checksum, compatibility, or evidence artifact.
- Routine CI performs no release-mode cross-target build.
- Routine CI does not install `cargo-audit`, language servers, or WASM targets.
- Test and build concurrency are explicitly bounded.
- Essential static guards still fail the job when their contract is violated.

Deferred work:

- Canonical quick/full local commands.
- Manual crates.io publication.
- Removal of the separate release and real-server workflows.

### Milestone 002 — Canonical local verification contract

Class: invariant

Objective:

Define one authoritative quick/full/change-specific verification contract and reconcile resource limits, test taxonomy, scripts, aliases, and contributor guidance around it.

Dependencies:

- Milestone 001 closed.

Deliverable boundary:

- Add or designate one small verification entry point.
- Define quick and full modes without introducing a new orchestration framework.
- Restore truthful terminology and conservative test/build limits.
- Make Nextest optional and remove redundant profiles or scripts that are not part of the canonical path.
- Document change-triggered feature, plugin, LSP, example, audit, and platform checks.
- Reconcile `AGENTS.md`, `CONTRIBUTING.md`, and `architecture/testing.md`.

User or operator value:

- Contributors know what to run during iteration and before handoff.
- Maintainers can obtain broader confidence without forcing that cost onto every commit.
- Resource-heavy test behavior is predictable on constrained development machines.

Exit conditions:

- Quick and full verification have one authoritative definition.
- Full workspace execution uses explicit bounded build and test concurrency.
- No documentation describes fourteen test threads as serial.
- Optional checks are tied to concrete change triggers.
- CI invokes a defined subset of the same verification contract rather than a separate policy.

Deferred work:

- Crates.io package publication inventory and release checklist.
- Final compatibility/evidence workflow deletion.

### Milestone 003 — Manual crates.io release ownership

Class: capability

Objective:

Remove automated GitHub release ownership and establish a reproducible maintainer-operated crates.io release procedure with explicit package scope and dry-run evidence.

Dependencies:

- Milestone 002 closed.
- Workspace package metadata can be inspected and corrected without changing runtime architecture.

Deliverable boundary:

- Delete the tag-triggered release workflow.
- Add a concise maintainer release document.
- Inventory publishable and private workspace packages.
- Correct package metadata required for the intended crates.io publication scope.
- Define dependency order, dry-runs, immutable-version failure handling, tagging, and optional manual GitHub Release behavior.
- Ensure no repository workflow contains registry credentials or publication commands.

User or operator value:

- Release cadence is controlled directly by the maintainer.
- Publishing failures are isolated from ordinary development.
- Version immutability and package ordering are explicit rather than repaired through CI iterations.

Exit conditions:

- No tag, push, pull request, schedule, or workflow dispatch publishes or creates a release.
- Intended packages pass `cargo package` or `cargo publish --dry-run` under the documented procedure.
- Internal packages are explicitly non-publishable or have complete publish metadata.
- GitHub tags and releases, if used, are optional manual follow-up operations.
- `package.metadata.binstall` is either aligned with a maintained manual binary-release process or removed/documented as unsupported.

Deferred work:

- Final removal of LSP evidence aggregation and obsolete helper scripts.

### Milestone 004 — Optional integration evidence cleanup and closure

Class: polish

Objective:

Retire the remaining scheduled and artifact-producing verification apparatus, preserve useful opt-in commands, and leave one internally consistent maintenance contract.

Dependencies:

- Milestones 001–003 closed.

Deliverable boundary:

- Remove scheduled and push-triggered real-server compatibility automation.
- Prefer local real-server commands; retain a manual workflow only if a concrete need is proven and the workflow is reduced to direct smoke execution without artifact aggregation.
- Remove orphaned compatibility-manifest and timing-evidence scripts.
- Collapse unused Nextest profiles and stale CI terminology.
- Reconcile workflow count, documentation, package metadata, and planning state.
- Measure and record the final routine-CI command surface and remaining optional checks.

User or operator value:

- The repository no longer accumulates verification infrastructure that requires its own maintenance roadmap.
- Expensive subsystem checks remain available without blocking unrelated work.
- Future contributors can understand the complete CI/release policy from a small set of files.

Exit conditions:

- Exactly one routine GitHub Actions workflow remains unless a narrowly justified manual-only diagnostic workflow is documented.
- No scheduled compatibility job or evidence aggregation job remains.
- No orphaned workflow-only script or profile remains.
- All verification and release documentation agrees on ownership, commands, and resource limits.
- Closure evidence demonstrates that removed automation did not delete the underlying tests or local commands.

Deferred work:

- Future product-level external CI runner adapters.
- Supply-chain signing or package-distribution expansion.

## 8. Cross-cutting requirements

### Storage and migration

This workstream must not add production storage or schema migrations. Test databases may be used by existing tests, but verification scripts must not mutate durable user state.

### Protocol and compatibility

No production protocol changes are expected. Optional compatibility harnesses must remain callable through their existing feature gates or documented replacements.

### Security and authorization

- No registry token or GitHub release credential may be committed or required by routine CI.
- Publication commands must rely on maintainer-local Cargo authentication.
- Pull-request code from untrusted forks must never gain a path to publication.
- Removing `cargo audit` from routine CI must not remove the documented manual audit command.
- Static security and ownership guards required by changed code must remain available and actionable.

### Concurrency, cancellation, and recovery

- `CARGO_BUILD_JOBS` and Rust test thread counts must be explicit for broad verification.
- Commands must terminate with the underlying process status and must not hide failures in aggregation scripts.
- Optional external-server tests must have bounded timeouts and cleanup inherited from their owning harness.
- Interrupted publication must be handled according to crates.io immutability: already-published versions are never retried as mutable replacements.

### Observability and audit

Routine CI logs are the evidence surface. Artifact manifests and duplicated diagnostic lanes are not required. Local full verification should print the command tier and failing command clearly.

### Performance and resource use

- Minimize runner count and repeated compilation.
- Avoid release-mode builds during ordinary validation.
- Avoid installing tools from source in routine CI.
- Avoid broad all-features execution unless the selected tier or change trigger requires it.
- Preserve conservative local settings for constrained hosts.

### Documentation and operations

The canonical policy must be discoverable from:

- `.github/workflows/ci.yml` for hosted behavior;
- one developer-facing verification document or script;
- `RELEASING.md` for maintainer publication;
- concise references from `AGENTS.md` and `CONTRIBUTING.md`.

Historical closure documents may retain old commands for traceability, but active guidance must not point contributors to obsolete workflows.

## 9. Verification strategy

Subsystem closure requires contract-level evidence rather than an exhaustive historical evidence archive:

- parse and inspect the final workflow triggers and job graph;
- run the routine verification commands locally where feasible;
- run quick and full verification tiers with recorded exit status;
- run representative change-specific commands for plugins and LSP;
- verify release workflow absence and search for `cargo publish`, `gh release create`, registry secrets, tag release triggers, and artifact upload steps under `.github/workflows`;
- run package dry-runs for every intended publishable crate;
- confirm optional tests still exist after workflow/script deletion;
- confirm broad test execution uses explicit resource bounds;
- record any environment-dependent command as not run rather than manufacturing evidence.

No milestone may claim reduced runtime or runner usage solely from line-count reduction. Closure should compare the before/after workflow job graph and command inventory.

## 10. Risks and decision points

- A single CI job may have less parallel wall-clock speed than several jobs, but repeated setup and duplicated compilation currently dominate. The milestone must optimize total feedback and maintenance cost, not maximize dashboard parallelism.
- Default-feature routine tests may not cover every optional feature. The verification contract must map optional features to change-specific local checks rather than silently dropping them.
- Some workspace packages may not currently be crates.io-ready because of path dependencies or stale metadata. Milestone 003 must stop and report rather than broadening publication scope without an explicit package decision.
- Removing scheduled real-server tests reduces automatic compatibility drift detection. This is accepted in favor of intentional subsystem verification; the underlying harness must remain available.
- A manual-only LSP workflow may still be useful on clean hosted machines. It may survive only if it is substantially simpler than the current matrix and has no scheduled, push, aggregation, or artifact-retention behavior.
- Existing branch-protection rules may name removed job checks. Repository administration changes may be required after CI contraction and must be reported if not available to the implementation agent.
- Historical documents will contain obsolete commands. Active docs should be corrected; historical closure records should not be rewritten merely for cosmetic consistency.

No ADR is currently required. Create one only if implementation proposes automated release authority, a mandatory third-party verification service, or a new canonical build system.

## 11. Completion definition

This roadmap is closed only when:

- routine GitHub Actions consists of one bounded non-release verification path;
- quick, full, and change-specific verification are canonically documented and internally consistent;
- broad test/build concurrency is explicit and conservative;
- release cadence, versioning, crates.io publication, tags, and optional GitHub releases are maintainer-operated;
- no workflow can publish or create a release;
- intended publishable packages have a documented successful dry-run path;
- expensive compatibility, audit, cross-target, and example checks are opt-in;
- scheduled real-server evidence production and aggregation are removed;
- obsolete scripts/profiles are removed or clearly retained for a current use;
- active repository documentation agrees with the final behavior;
- closure records for all four milestones contain actual command and repository evidence.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 001 — Routine CI contraction | closed | `plans/implementation/development-verification-release/001-routine-ci-contraction.md` | `plans/closure/development-verification-release/001-status.md` | — |
| 002 — Canonical local verification contract | ready | `plans/implementation/development-verification-release/002-local-verification-contract.md` | — | — |
| 003 — Manual crates.io release ownership | blocked | `plans/implementation/development-verification-release/003-manual-crates-io-release-ownership.md` | — | Milestone 002 closure |
| 004 — Optional integration evidence cleanup and closure | blocked | `plans/implementation/development-verification-release/004-integration-evidence-cleanup-and-closure.md` | — | Milestone 003 closure |
