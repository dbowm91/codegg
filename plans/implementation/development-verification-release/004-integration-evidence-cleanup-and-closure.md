# Development Verification and Release Milestone 004 — Optional Integration Evidence Cleanup and Closure

Status: closed

Repository baseline: `39d0720f9748cabc978ad9b0a3a32c31c6bc84d1` plus the development-verification-release planning registration series

Source roadmap:

- `plans/subsystems/development-verification-release-roadmap.md#milestone-004--optional-integration-evidence-cleanup-and-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#3-non-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- None.

Primary class: polish

## 1. Objective

Remove the remaining scheduled, push-triggered, artifact-producing, and evidence-aggregation verification apparatus; preserve valuable integration tests as explicit local or narrowly justified manual commands; remove orphaned helper scripts and profiles; reconcile all active documentation; and produce final closure evidence for the development-verification-release roadmap.

The desired endpoint is one ordinary GitHub Actions workflow and a small local/manual command surface. A manual-only diagnostic workflow may remain only when the implementation agent demonstrates a concrete need that cannot be met reasonably by local commands and reduces it to direct smoke execution without schedules, push triggers, artifact aggregation, retention policy, or release authority.

## 2. Why this milestone is ready

This plan becomes ready only after:

- Milestone 001 has established one lean routine CI path;
- Milestone 002 has established canonical quick/full/change-specific local verification;
- Milestone 003 has deleted automated release ownership and established manual crates.io procedure.

Those dependencies provide the replacement surfaces needed to remove the remaining compatibility and evidence automation without losing operational capability.

No production architecture decision remains. This is a final maintenance and consistency pass.

## 3. Current implementation evidence

At the reviewed baseline, `.github/workflows/lsp-real-server.yml` has three trigger classes:

- manual dispatch;
- a weekly Monday schedule;
- pushes to `main` touching `crates/egglsp/**`, `src/lsp/**`, or the workflow itself.

It contains five server-specific jobs:

- rust-analyzer;
- basedpyright;
- gopls;
- TypeScript language server;
- clangd.

Each job provisions external tools, runs one selected real-server smoke path, and uploads a compatibility report artifact with a retention period. A sixth `matrix-summary` job downloads all reports, runs `scripts/aggregate_lsp_compatibility_manifest.py`, validates expected commit/run metadata, and uploads a 90-day aggregate manifest.

The useful product capability is the real-server smoke harness under `crates/egglsp/tests/real_server_smoke.rs` and its server-selector commands. The expensive operational apparatus is the scheduled/push matrix, provisioning duplication, artifact handling, retention policy, and manifest aggregation.

The repository also contains optional Nextest profiles and timing/evidence scripts. Milestone 002 owns the canonical policy and initial simplification; this milestone must remove any remaining helper whose only live consumer was old CI or closure-evidence production.

Active documentation is distributed across at least:

- `architecture/testing.md`;
- `architecture/lsp.md`;
- `AGENTS.md`;
- `CONTRIBUTING.md`;
- workflow comments;
- Nextest configuration/scripts;
- package/release guidance added by Milestone 003.

Final closure requires these surfaces to describe one consistent system.

## 4. Invariants that must not regress

- Real-server LSP smoke tests must remain runnable for each supported server selector.
- Fake-server deterministic LSP tests must remain part of the documented local/full or change-specific contract.
- Removing a workflow must not delete production LSP behavior, test fixtures, protocol assertions, or server adapters.
- Routine CI must remain the single required hosted gate.
- No scheduled external compatibility job may remain.
- No compatibility report aggregation or retention apparatus may remain without a current named operator need.
- No GitHub Actions workflow may regain release authority.
- Optional commands must report what was actually run; absence of an artifact is not a test failure.
- Orphan deletion must be reference-driven. Do not delete a script merely because its name contains `evidence`, `report`, `timing`, or `compatibility`.
- Active documentation must agree on verification tiers, resource bounds, release ownership, workflow count, and optional checks.
- Historical plans and closure records must remain intact for traceability.

## 5. Scope

### In scope

- `.github/workflows/lsp-real-server.yml` deletion or exceptional manual-only contraction;
- removal of schedule and push triggers for real-server compatibility;
- removal of per-server artifact upload/download and retention configuration;
- removal of `matrix-summary` aggregation;
- reference audit and deletion of `scripts/aggregate_lsp_compatibility_manifest.py` if orphaned;
- reference audit and deletion/simplification of remaining CI-evidence and Nextest timing helpers;
- final cleanup of `.config/nextest.toml` after Milestone 002;
- active documentation reconciliation;
- final repository searches for workflow, release, artifact, schedule, test-thread, and stale-command drift;
- final roadmap/registry/closure-state updates after implementation and independent review.

### Explicitly out of scope

- deleting `real_server_smoke.rs` or supported server selectors;
- removing LSP support from CodeGG;
- changing LSP protocol semantics to reduce test count;
- adding a new compatibility SaaS or external dashboard;
- replacing removed reports with a database or long-lived local evidence store;
- adding benchmark, code coverage, supply-chain, signing, or release automation;
- restoring multi-platform builds to routine CI;
- changing crates.io publication scope established by Milestone 003;
- rewriting historical closure evidence;
- closing unrelated subsystem roadmaps.

## 6. Required production changes

### Core/domain

No production domain changes are expected.

Do not modify LSP runtime code unless a retained smoke command exposes an independently existing correctness defect. Such a defect must be reported and moved to the owning LSP workstream rather than silently absorbed into this cleanup plan.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

Preferred workflow outcome:

```text
.github/workflows/ci.yml          retained as the only ordinary workflow
.github/workflows/release.yml     absent from Milestone 003
.github/workflows/lsp-real-server.yml absent
```

The canonical real-server commands belong in the Milestone 002 change-specific verification matrix, for example:

```bash
cargo test -p egglsp \
  --features lsp-real-server-tests \
  --test real_server_smoke \
  -- rust_analyzer --nocapture
```

Equivalent commands must be listed for basedpyright, gopls, TypeScript, and clangd where still supported by the harness.

If a manual-only hosted workflow is retained as an exception, it must satisfy all of these constraints:

- only `workflow_dispatch` trigger;
- no `schedule`;
- no `push` or `pull_request` trigger;
- no matrix-summary job;
- no artifact upload/download;
- no retention configuration;
- no expected-run/commit manifest aggregation;
- no release permissions;
- preferably one job that accepts or defines a small explicit server selector and runs sequentially;
- a documented reason local commands are insufficient.

Do not retain five nearly identical jobs merely because they already exist.

### Frontend or operator surface

The operator surface becomes documentation and direct command output. The smoke tests should print server version and failure diagnostics through their existing harness. No generated compatibility manifest is required.

### Security and authorization

- External tool installation remains an explicit maintainer action or an isolated manual job.
- No scheduled download of third-party binaries.
- Keep existing checksum verification in local installation documentation where a downloaded archive is used.
- Do not grant workflow write permissions.
- Do not upload logs that may contain repository paths or source snippets merely for evidence retention.

### Documentation and static guards

Reconcile active docs to state:

- one routine workflow exists;
- release is manual and crates.io-oriented;
- quick/full verification is local and bounded;
- real-server, plugin examples, audit, packaging, and platform checks are opt-in;
- real-server compatibility is run intentionally when LSP code changes or before a release;
- no weekly evidence matrix exists.

Use repository searches and existing tests for policy verification. Do not create a new permanent “CI architecture validator” unless a current simple guard already exists and can absorb one or two high-value assertions.

## 7. Ordered work packages

### Work package A — Inventory remaining workflow and evidence consumers

Intent:

Prove which files are operationally live before deleting anything.

Required changes:

- List all `.github/workflows/*.yml` and `.yaml` files after Milestone 003.
- Search active and historical references to:
  - `lsp-real-server.yml`;
  - `aggregate_lsp_compatibility_manifest.py`;
  - `target/lsp-compatibility`;
  - `lsp-compat-matrix-manifest`;
  - Nextest CI profile names;
  - timing/evidence capture scripts;
  - artifact upload/download actions.
- Classify each reference as production harness, active operator documentation, workflow-only helper, historical evidence, or orphan.

Acceptance evidence:

- A reference inventory in the closure record.
- Every deleted file is shown to have no active consumer after planned documentation changes.
- Production test-harness files are clearly separated from workflow-only aggregation files.

### Work package B — Retire the real-server workflow apparatus

Intent:

Keep compatibility testing while removing automatic and artifact-heavy execution.

Required changes:

Preferred path:

- delete `.github/workflows/lsp-real-server.yml`;
- retain all real-server selectors and test code;
- add/update local commands in `architecture/lsp.md`, `architecture/testing.md`, and the canonical verification matrix.

Exceptional path:

- reduce the workflow to manual dispatch only;
- use one simple job or a tightly bounded selector;
- remove all artifacts, aggregation, schedules, push triggers, and retention.

The exceptional path requires a written repository-specific justification in the implementation commit and closure record.

Acceptance evidence:

- No scheduled or push-triggered real-server execution remains.
- Every previously supported server has a runnable direct command or an explicit documented deprecation owned by LSP maintainers.
- No test source is deleted.

### Work package C — Remove orphaned aggregation and timing machinery

Intent:

Delete infrastructure whose only purpose was to support removed hosted evidence production.

Required changes:

- Delete `scripts/aggregate_lsp_compatibility_manifest.py` if no active non-workflow consumer remains.
- Remove tests dedicated solely to that script if the script is deleted and those tests do not validate production report formats.
- Remove compatibility-manifest docs that no longer describe an active contract.
- Finish Nextest/profile cleanup started by Milestone 002:
  - remove unused `ci-*` profiles;
  - retain at most profiles with a current named local use;
  - delete timing capture scripts if they are no longer used outside historical closure work.
- Remove obsolete artifact directory expectations from active docs and `.gitignore` only when safe.

Acceptance evidence:

- Repository search shows no active dangling reference.
- Deleted scripts are not imported or invoked by production code/tests.
- Any retained report format has a current test or operator use.

### Work package D — Reconcile active documentation

Intent:

Leave one compact and internally consistent operational model.

Required changes:

Review and update:

- `README.md`;
- `AGENTS.md`;
- `CONTRIBUTING.md`;
- `architecture/testing.md`;
- `architecture/lsp.md`;
- `RELEASING.md`;
- `.github/workflows/ci.yml` comments;
- `.config/nextest.toml` comments if retained;
- any root badges or status references.

Required content:

- exact routine workflow scope;
- quick/full entry points;
- one-thread broad test policy;
- change-specific real-LSP/plugin/example/audit/platform commands;
- manual crates.io release ownership;
- no promise of tag-triggered GitHub binaries unless Milestone 003 explicitly retained a manual binary-release process.

Acceptance evidence:

- Consistency search results.
- No active document points at a deleted workflow/script/profile.
- Historical closure records remain untouched.

### Work package E — Final lean-apparatus verification

Intent:

Prove the end state rather than merely deleting files.

Required changes:

- Run Milestone 002 quick and full verification.
- Run fake-server LSP integration.
- Run at least one available real-server smoke command on the implementation host; run additional servers when already installed without creating a large provisioning exercise.
- Run plugin/example or audit commands only as required by changed files and record which were not run.
- Search workflows for schedules, matrices, artifacts, release commands, and write permissions.
- Count remaining workflow jobs and classify each.
- Compare before/after workflow runner/job surface against the roadmap baseline.

Acceptance evidence:

- Exact command results.
- Final workflow inventory.
- Proof of retained optional test code.
- Honest environment limitations.

### Work package F — Close planning state

Intent:

Make the roadmap and registry reflect implementation reality after independent closure review.

Required changes:

- Create `plans/closure/development-verification-release/004-status.md` only during independent closure review.
- Link prior milestone closure records.
- Mark the subsystem roadmap closed only if all four milestones are strictly closed.
- Move the registry row from active to recently closed according to registry rules.
- Keep residual optional future work unregistered unless it has an approved new roadmap.

Acceptance evidence:

- Roadmap, registry, implementation plans, and closure records agree.
- No “closed” claim precedes independent evidence review.

## 8. Failure, cancellation, restart, and contention semantics

Deleting scheduled automation means compatibility checks occur only when intentionally invoked. This is accepted policy, not a failure requiring replacement monitoring.

Local smoke commands may be rerun from the beginning after interruption. No manifest resume or artifact download is required.

If an external server is unavailable:

- report it as not run or skipped according to the harness;
- do not install a large toolchain solely to manufacture closure evidence unless that server is already a documented release prerequisite;
- do run deterministic fake-server coverage.

If a retained manual workflow is cancelled, direct test output is sufficient. Do not upload partial reports through `if: always()`.

Concurrent real-server smoke runs are unnecessary. Run selected servers sequentially to limit process and memory load.

## 9. Compatibility and migration

No production protocol or data migration is involved.

Operator migration:

- weekly and path-triggered LSP compatibility runs disappear;
- maintainers use documented local commands or the exceptional manual-only workflow;
- compatibility artifacts and matrix manifests are no longer generated;
- release workflow remains absent;
- routine CI remains the only automatic hosted verification.

Links or badges pointing at deleted workflow names must be updated or removed.

Do not remove feature flags or test selectors used by local commands.

## 10. Required tests

### Focused unit tests

Retain and run tests for real-server selector parsing/report generation when those components remain part of the harness. Remove only tests whose subject is a deleted workflow-only aggregation script.

### Integration tests

- canonical quick verification;
- canonical full verification;
- fake-server LSP scenario tests;
- root fake-LSP stdio integration;
- at least one available real-server smoke selector;
- any documentation or script checks added by Milestone 002.

### Restart and recovery tests

Not applicable to repository workflow state. Confirm direct smoke commands leave no persistent verification state that must be recovered.

### Contention and cancellation tests

No new stress test required. Use sequential real-server execution and preserve existing process cleanup tests.

### Security and negative tests

- no workflow schedule except a separately justified unrelated current workflow;
- no artifact upload/download in optional integration verification;
- no workflow release commands or write permissions;
- no dangling active reference to deleted scripts;
- no credentials or tokens in commands/docs;
- no real-server invocation in quick verification.

### Migration and compatibility tests

- all supported real-server selectors remain discoverable;
- active links point to existing files;
- any retained manual workflow can be dispatched without artifact assumptions;
- documentation clearly describes the loss of automatic compatibility runs.

## 11. Required verification commands

Canonical tiers:

```bash
scripts/verify.sh quick
scripts/verify.sh full
```

Deterministic LSP integration:

```bash
CARGO_BUILD_JOBS=1 cargo test -p egglsp --locked --features lsp-test-support --test scenario_engine -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --locked --features lsp-test-support --test lsp_composite_stdio -- --test-threads=1
```

At least one installed real server:

```bash
CARGO_BUILD_JOBS=1 cargo test -p egglsp --locked \
  --features lsp-real-server-tests \
  --test real_server_smoke \
  -- rust_analyzer --nocapture
```

Use the final documented selector for another installed server where practical.

Workflow inventory and policy searches:

```bash
find .github/workflows -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) -print | sort

rg --line-number --glob '.github/workflows/*.{yml,yaml}' \
  'schedule:|upload-artifact|download-artifact|retention-days|matrix-summary|cargo publish|gh release create|contents: write|packages: write|id-token: write' \
  .github/workflows
```

Reference searches:

```bash
rg --line-number --glob '!plans/archive/**' --glob '!plans/closure/**' \
  'lsp-real-server\.yml|aggregate_lsp_compatibility_manifest|lsp-compat-matrix-manifest|ci-fast|ci-heavy|ci-release|capture-nextest-timing' \
  .

rg --line-number --glob '!plans/archive/**' --glob '!plans/closure/**' \
  'test-threads=14|weekly.*LSP|tag.*release|automatic.*release' \
  README.md AGENTS.md CONTRIBUTING.md architecture plans/subsystems plans/implementation .github .config scripts
```

Retained test proof:

```bash
test -e crates/egglsp/tests/real_server_smoke.rs
rg --line-number 'rust_analyzer|basedpyright|gopls|typescript|clangd' crates/egglsp/tests architecture/lsp.md architecture/testing.md
```

Adjust paths only when current repository evidence has legitimately moved them.

## 12. Documentation updates

Required final reconciliation:

- `README.md` — badges/distribution/verification pointers;
- `AGENTS.md` — canonical commands and focused LSP examples;
- `CONTRIBUTING.md` — contributor and maintainer expectations;
- `architecture/testing.md` — final tiers and resource model;
- `architecture/lsp.md` — local real-server compatibility procedure;
- `RELEASING.md` — optional pre-release compatibility checks, no automated release claim;
- subsystem roadmap and registry after closure.

Do not preserve obsolete active prose merely to minimize diff size. Do preserve historical implementation and closure records.

## 13. Acceptance criteria

- `.github/workflows/release.yml` remains absent.
- `.github/workflows/lsp-real-server.yml` is absent, or an explicitly justified manual-only version has only `workflow_dispatch` and no artifact/aggregation behavior.
- No scheduled or push-triggered real-server compatibility execution remains.
- No `matrix-summary` compatibility aggregation remains.
- No per-server compatibility artifact upload/download or retention remains.
- `scripts/aggregate_lsp_compatibility_manifest.py` is deleted if orphaned.
- Remaining Nextest profiles/scripts have a current named local use; no stale CI profile remains.
- Quick and full verification pass or independently existing failures are precisely documented.
- Fake-server LSP integration remains and is run.
- At least one available real-server selector is run successfully or an environment limitation is recorded without false evidence.
- Real-server test source and supported selectors remain.
- Active documentation agrees on one routine workflow, one-thread broad tests, opt-in expensive checks, and manual crates.io release.
- No active link points to a deleted workflow, script, profile, or artifact.
- The final workflow/job count is recorded and materially smaller than baseline.
- The roadmap is marked closed only after independent closure records for all four milestones exist.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Milestone 003 is not strictly closed;
- a purported workflow-only script is imported by production code or validates a public report contract;
- removing the LSP workflow would eliminate the only runnable path for a supported server rather than merely hosted provisioning;
- a concrete maintainer requirement mandates recurring compatibility monitoring contrary to this roadmap;
- active external badges, integrations, or consumers require an artifact contract that is not documented in the repository;
- deterministic fake-LSP tests fail because of a product defect outside this cleanup scope;
- a real-server failure indicates an LSP correctness regression requiring its own implementation plan;
- cleanup would require rewriting historical closure records;
- unrelated user changes cannot be preserved.

## 15. Closure evidence required

The final closure record must contain:

- closure links for Milestones 001–003;
- before/after workflow inventory and job count;
- trigger inventory;
- artifact/aggregation removal evidence;
- reference classification for every deleted script/profile;
- final quick/full/change-specific command contract;
- quick/full/fake-LSP/available-real-LSP command results;
- explicit optional commands not run and why;
- proof that real-server test source and selectors remain;
- active documentation consistency search results;
- proof no automated release authority returned;
- residual risks from intentional removal of scheduled compatibility monitoring;
- final roadmap and registry disposition;
- independent reviewer conclusion.

## 16. Handoff notes

This is a deletion and reconciliation milestone, not an invitation to replace old evidence machinery with new evidence machinery.

The real-server harness is valuable. The weekly matrix, per-server artifact retention, and aggregate manifest are the maintenance burden. Preserve the former and remove the latter.

Use reference searches before deletion and after documentation updates. A file is orphaned only when no active code, test, workflow, or operator document consumes it.

Keep the final operational story small enough to explain in a few paragraphs: one routine CI job, bounded local quick/full verification, explicit change-specific checks, and manual crates.io release.
