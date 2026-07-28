# Development Verification and Release Milestone 003 — Manual crates.io Release Ownership

Status: implemented

Repository baseline: `39d0720f9748cabc978ad9b0a3a32c31c6bc84d1` plus the development-verification-release planning registration series

Source roadmap:

- `plans/subsystems/development-verification-release-roadmap.md#milestone-003--manual-cratesio-release-ownership`

Long-term requirements:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#3-non-goals`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- None. Maintainer-operated release ownership is the explicit policy for this workstream.

Primary class: capability

## 1. Objective

Remove GitHub Actions from release ownership and establish a reproducible, maintainer-operated crates.io publication procedure with explicit package scope, dependency order, package metadata, dry-run gates, immutable-version failure handling, tagging, and optional manual GitHub Release behavior.

The milestone must make it impossible for a repository workflow to publish crates or create releases while keeping publication practical for the maintainer. It must not invent an automated replacement release system.

## 2. Why this milestone is ready

This plan becomes ready only after Milestone 002 closes and the canonical full local verification contract exists. Release preparation must consume that contract rather than creating another release-only verification framework.

The maintainer has already decided:

- release cadence is manual;
- crates.io publication is performed separately from GitHub CI;
- ordinary CI must not be delayed or destabilized by release packaging;
- GitHub Actions must not create releases or own publication credentials.

The remaining work is repository policy, package metadata, and procedural verification. No runtime architecture decision is required.

## 3. Current implementation evidence

At the reviewed baseline, `.github/workflows/release.yml` triggers on tags matching `v*` and:

- builds Linux x86-64 and ARM64 binaries;
- builds Intel and Apple Silicon macOS binaries;
- builds a Windows x86-64 binary;
- installs `cross` for ARM64 Linux;
- uploads each binary as an Actions artifact;
- downloads artifacts into a checksum job;
- generates SHA-256 checksums;
- downloads everything again into a release job;
- runs `gh release create` with generated notes and uploaded assets.

The workflow does not publish crates.io packages. It couples a Git tag to GitHub binary release creation and therefore imposes an automated release path the maintainer no longer wants.

The root `Cargo.toml` at the reviewed baseline contains:

- package name `codegg` and version `0.1.0`;
- repository/homepage metadata pointing at `anomalyco/codegg` rather than the current repository;
- `package.metadata.binstall` URLs that assume GitHub release assets exist for five targets;
- path dependencies on internal workspace crates without visible version constraints in the root manifest;
- a ten-member workspace whose individual publication intent is not centrally documented.

A crates.io release cannot be considered reproducible until the workspace package graph is classified. A path-only internal dependency is not sufficient for a published package unless the dependency is also available from crates.io and the manifest supplies a compatible version. Internal packages not intended for publication should be explicitly marked `publish = false`.

## 4. Invariants that must not regress

- No GitHub Actions trigger may publish a crate, create a tag, create a GitHub Release, upload release assets, or choose a version.
- No crates.io token or release credential may be stored in repository workflows or configuration.
- Release cadence remains a maintainer decision.
- Every intended publishable package must have complete crates.io metadata and a successful package/dry-run path.
- Every internal-only package must be explicitly non-publishable or clearly excluded from the release procedure.
- Published workspace dependencies must use compatible registry versions in addition to local paths where necessary.
- Publication order must follow the actual dependency graph.
- Already-published crate versions are immutable and must never be treated as replaceable artifacts.
- A partial release must not be repaired by republishing the same version; the procedure must require a new version where correction is needed.
- Optional GitHub tags or releases must remain manual and must not be prerequisites for crates.io publication.
- Removing automated binary releases must not leave misleading `binstall` metadata that points to assets no longer produced.
- The release procedure must consume Milestone 002 full verification rather than duplicating its commands in a divergent script.

## 5. Scope

### In scope

- `.github/workflows/release.yml` deletion;
- a new root `RELEASING.md` or equivalently discoverable maintainer document;
- workspace publication inventory;
- package metadata corrections required for the intended crates.io scope;
- explicit `publish = false` for private packages where appropriate;
- path-plus-version dependency declarations for publishable internal dependencies where appropriate;
- package and publish dry-run commands;
- dependency-order publication instructions;
- crates.io index propagation and immutable-version handling;
- changelog/version/tag guidance;
- optional manual GitHub Release guidance;
- disposition of `package.metadata.binstall`;
- static search evidence that no workflow owns release operations.

### Explicitly out of scope

- publishing an actual release unless the maintainer separately instructs the implementation agent to do so;
- choosing the next version number;
- making all workspace crates public by default;
- renaming crates to solve registry-name conflicts without maintainer approval;
- adding release-plz, cargo-release, cargo-workspaces, semantic-release, goreleaser, cross-rs release automation, or another release framework;
- signing, provenance, SBOM, notarization, installers, Homebrew, AUR, Debian/RPM, or container publication;
- automatic changelog generation;
- automatic GitHub tags or releases;
- changing production APIs solely to make packaging convenient;
- deleting the LSP compatibility workflow;
- final cleanup of evidence-only scripts owned by Milestone 004.

## 6. Required production changes

### Core/domain

No runtime domain behavior changes are expected. Package metadata changes must not alter public Rust APIs unless unavoidable and explicitly reported.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

Release commands run manually and sequentially. The procedure must use the closed Milestone 002 full verification entry point before package dry-runs.

The guide must not instruct maintainers to run several `cargo publish` commands concurrently. Crates.io index visibility may lag after publishing a dependency; the guide must include a bounded verification/wait step before publishing a dependent crate.

### Frontend or operator surface

Add `RELEASING.md` with a compact but complete operator procedure. It must identify:

1. prerequisites;
2. package scope and dependency order;
3. clean-tree/mainline preflight;
4. version and changelog update expectations;
5. full local verification;
6. package contents inspection;
7. `cargo publish --dry-run` sequence;
8. actual publish sequence;
9. crates.io index propagation handling;
10. tag creation and push;
11. optional manual GitHub Release behavior;
12. partial-failure, yanking, and new-version correction policy.

The guide must clearly distinguish commands that verify from commands that irreversibly publish.

### Security and authorization

- Cargo authentication remains in the maintainer's local Cargo credential store or environment.
- Do not document literal tokens.
- Do not add repository secrets.
- Do not add GitHub OIDC trusted publishing in this milestone.
- Do not echo credentials.
- Recommend verifying the active crates.io account and package ownership before the irreversible step.
- Commands copied into the guide must not accidentally publish during dry-run sections.

### Documentation and static guards

Delete `.github/workflows/release.yml` after the manual procedure and package dry-runs are established.

Update active references that claim tag pushes automatically create release binaries or GitHub Releases.

Add a minimal policy statement to `README.md` or `CONTRIBUTING.md` only if release discovery would otherwise be unclear. The detailed procedure belongs in `RELEASING.md`.

Do not add a complex static guard script. Closure searches over `.github/workflows` are sufficient unless the repository already has a simple workflow-policy check that can be extended without new machinery.

## 7. Ordered work packages

### Work package A — Inventory the workspace publication graph

Intent:

Determine what is actually intended and technically possible to publish before editing metadata or deleting the old release path.

Required changes:

- Use `cargo metadata --format-version 1 --no-deps` and direct manifest review to list every workspace package.
- For each package, record:
  - package name and version;
  - manifest path;
  - `publish` policy;
  - description, license, repository, homepage, readme, keywords/categories where applicable;
  - internal path dependencies;
  - whether the package is intended public API, an implementation crate, a test fixture, or unpublished application support.
- Identify registry-name availability or ownership issues that cannot be resolved from repository content.
- Derive the dependency order for packages that are explicitly intended for publication.

Acceptance evidence:

- A publication inventory table in the closure record and concise package-scope table in `RELEASING.md`.
- No package is made public merely because it is a workspace member.
- Unresolved registry ownership/name questions are stop conditions, not guessed.

### Work package B — Correct package publication metadata

Intent:

Make the explicitly intended package set pass Cargo packaging rules while making private intent unambiguous.

Required changes:

- Correct stale repository/homepage metadata to the current canonical repository where the package is maintained here.
- Add `publish = false` to packages that are not intended for crates.io.
- For publishable packages that depend on other publishable workspace packages, use Cargo's path-plus-version form, for example:

```toml
codegg-core = { version = "0.1.0", path = "crates/codegg-core" }
```

The actual version must match the package being prepared; do not copy this example blindly.

- Ensure every publishable package has required description/license/repository/readme metadata and includes the files needed to compile from the packaged archive.
- Inspect `cargo package --list` for accidental secrets, local artifacts, huge fixtures, generated evidence, or missing source files.
- Avoid broad `include`/`exclude` churn unless package contents show a concrete problem.
- Decide the root `package.metadata.binstall` disposition:
  - retain only if maintainers intend to create matching binary assets manually and the guide documents the exact naming contract;
  - otherwise remove it or clearly mark binary-install metadata unsupported.

Acceptance evidence:

- Every intended package passes `cargo package --allow-dirty` during iterative editing and passes without `--allow-dirty` from the final clean revision.
- Every private package is explicitly classified.
- Packaged manifests resolve registry dependencies correctly.

### Work package C — Write the manual release procedure

Intent:

Make maintainer publication deterministic without automating it.

Required changes:

Write `RELEASING.md` around a two-stage process.

#### Reversible preparation

```bash
git switch main
git pull --ff-only
git status --short
scripts/verify.sh full
cargo package -p <package>
cargo publish --dry-run -p <package>
```

The final guide must replace placeholders with the actual package sequence or explicitly state that only one package is published.

Preparation must also include:

- version consistency checks;
- changelog/release-note expectations if the repository maintains them;
- inspection of `cargo package --list`;
- crates.io ownership/account confirmation;
- optional manual audit or platform checks based on changed surfaces;
- a final clean-tree check after version edits.

#### Irreversible publication

```bash
cargo publish -p <dependency-package>
# verify crates.io/index visibility
cargo publish -p <dependent-package>
```

The guide must state:

- do not rerun the same version after successful publication;
- if a later package fails, fix it under a new version as required;
- use `cargo yank` only for a published broken version and understand that yanking is not deletion;
- tags should be created only after the intended publish state is confirmed, unless the maintainer explicitly chooses another documented order;
- optional GitHub Release creation is manual and separate from crates.io.

Acceptance evidence:

- Another maintainer or implementation agent can follow the dry-run path without reading the deleted workflow.
- Destructive commands are clearly labeled.

### Work package D — Remove GitHub release automation

Intent:

Transfer release authority completely out of GitHub Actions.

Required changes:

- Delete `.github/workflows/release.yml`.
- Remove active documentation describing tag-triggered binary releases.
- Search all workflow files for publication, release, tag, packages-write, contents-write, and artifact staging commands.
- Ensure routine CI remains read-only.
- Do not replace the deleted workflow with `workflow_dispatch` publication.

Acceptance evidence:

- `.github/workflows/release.yml` is absent.
- No `.github/workflows/*.yml` or `.yaml` file contains an executable `cargo publish` or `gh release create` path.
- No workflow has release-specific write permissions or crates.io credentials.

### Work package E — Prove dry-run packaging and partial-failure semantics

Intent:

Verify the manual process before declaring automated release removal complete.

Required changes:

- Run `cargo package` for each intended publishable crate in dependency order.
- Run `cargo publish --dry-run` for each intended publishable crate where crates.io registry resolution permits.
- Inspect generated package contents.
- Simulate/document a dependent-package dry-run failure after a dependency would have been published and ensure the guide requires a new version rather than overwriting.
- Confirm private packages are not accidentally selected by workspace-wide publication commands.

Acceptance evidence:

- Exact dry-run command results.
- Any registry/network limitation is reported honestly.
- No actual publish occurs as part of implementation unless separately authorized.

## 8. Failure, cancellation, restart, and contention semantics

Preparation commands are restartable. Actual crates.io publication is not transactional across several crates.

Required partial-release behavior:

```text
nothing published
  -> fix locally and rerun dry-runs

some dependency crates published, dependent crate failed before publication
  -> fix dependent package
  -> bump any version that must change or whose published dependency contract requires it
  -> rerun package and dry-run
  -> publish only versions not already present

published version is defective
  -> do not attempt replacement
  -> optionally yank the defective version
  -> prepare and publish a new version
```

Do not add automated rollback because crates.io publication is immutable. Git tags and GitHub Releases must not be created automatically as compensation.

Only one maintainer should execute the irreversible publish sequence at a time. The guide should recommend confirming no parallel release is underway.

Network or index propagation failure must be treated as an external state to verify, not a reason to blindly rerun `cargo publish`.

## 9. Compatibility and migration

Deleting the GitHub release workflow changes operator behavior:

- pushing a `v*` tag no longer creates release assets;
- `cargo-binstall` users may lose a binary asset path if `package.metadata.binstall` is removed or no manual binary release is produced;
- crates.io becomes the documented primary distribution path;
- optional GitHub tags/releases remain possible through manual commands.

Document the final binary-distribution stance explicitly. Do not retain metadata that advertises a nonexistent automated asset contract.

No production user-data migration is involved.

If workspace crates are made non-publishable, confirm that this matches existing public API intent. If a crate is already published, do not mark it private without documenting compatibility implications and maintainer approval.

## 10. Required tests

### Focused unit tests

No new Rust unit tests are required for release policy alone.

### Integration tests

- `cargo metadata` parses the final workspace.
- `cargo package` succeeds for intended packages.
- packaged archives compile through Cargo's package verification unless intentionally disabled with a documented reason.
- `cargo publish --dry-run` succeeds where registry access permits.

### Restart and recovery tests

Review the release guide's partial-publication state machine against at least one hypothetical failure after a dependency publish.

### Contention and cancellation tests

Confirm the guide warns against concurrent maintainers publishing the same release sequence.

### Security and negative tests

- Search workflows for release commands, write permissions, token names, and registry credentials.
- Confirm `RELEASING.md` contains placeholders such as `<package>` only in explanatory examples, not in the final executable sequence.
- Inspect `cargo package --list` for `.env`, credentials, local databases, target artifacts, and evidence bundles.
- Confirm private packages reject publication by policy where possible.

### Migration and compatibility tests

- Confirm active docs no longer promise automatic tag releases.
- Confirm final `binstall` metadata matches actual manual binary-release intent or is absent.
- Confirm package repository URLs point to the maintained repository.

## 11. Required verification commands

Inventory:

```bash
cargo metadata --format-version 1 --no-deps > /tmp/codegg-metadata.json
python3 - <<'PY'
import json
from pathlib import Path
m = json.loads(Path('/tmp/codegg-metadata.json').read_text())
for p in sorted(m['packages'], key=lambda p: p['name']):
    print(p['name'], p['version'], p['manifest_path'], p.get('publish'))
PY
```

Canonical verification dependency:

```bash
scripts/verify.sh full
```

For each intended publishable package, in dependency order:

```bash
cargo package -p <actual-package-name> --locked
cargo package -p <actual-package-name> --locked --list
cargo publish --dry-run -p <actual-package-name> --locked
```

Workflow-release absence:

```bash
test ! -e .github/workflows/release.yml
rg --line-number --glob '.github/workflows/*.{yml,yaml}' \
  'cargo publish|gh release create|crates\.io|CARGO_REGISTRY_TOKEN|packages: write|contents: write|id-token: write|tags:|upload-artifact|checksums' \
  .github/workflows
```

Review every search hit; some terms may be harmless in comments or non-release diagnostic workflows, but executable release authority is forbidden.

Metadata checks:

```bash
rg --line-number 'anomalyco/codegg|package\.metadata\.binstall|publish\s*=' \
  Cargo.toml crates/*/Cargo.toml README.md RELEASING.md
```

Do not run an actual `cargo publish` without separate explicit maintainer authorization.

## 12. Documentation updates

Required:

- add `RELEASING.md`;
- update `README.md` distribution/release references if present;
- update `CONTRIBUTING.md` maintainer release pointer if appropriate;
- update active architecture/testing documentation only where it claims release CI is part of verification;
- update package metadata repository/homepage/readme fields as required;
- remove references to the deleted workflow.

The release guide must be operational, not a historical narrative. Keep rationale concise and put exact package order and commands first.

## 13. Acceptance criteria

- `.github/workflows/release.yml` is deleted.
- No GitHub Actions workflow publishes crates, creates tags, creates GitHub Releases, or uploads release binaries/checksums.
- Workflow permissions contain no release-specific write authority.
- `RELEASING.md` defines a complete manual crates.io procedure.
- The package publication set is explicit.
- Private workspace crates are explicitly non-publishable or excluded with a documented reason.
- Publishable internal dependencies have registry-compatible version declarations.
- Stale repository/homepage metadata is corrected for intended packages.
- Every intended package passes `cargo package` and, where registry access permits, `cargo publish --dry-run`.
- Release commands use Milestone 002 full verification rather than a parallel release test suite.
- Partial publication and crates.io immutability are documented correctly.
- `binstall` metadata matches an intentional manual binary-release process or is removed/documented unsupported.
- No actual release is published without separate explicit authorization.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Milestone 002 is not strictly closed;
- intended publishable package scope cannot be inferred from existing public metadata and maintainer direction;
- a crate name is unavailable or owned by another crates.io account;
- a currently published crate would be made private or renamed;
- Cargo packaging requires a public API or workspace architecture redesign;
- path dependency conversion requires publishing an internal crate whose public status is unclear;
- registry access is unavailable and a dry-run result would be guessed;
- the repository contains an active non-GitHub release consumer that requires the deleted asset naming contract;
- actual publication would occur without explicit authorization;
- unrelated user changes cannot be preserved.

## 15. Closure evidence required

The closure record must contain:

- Milestone 002 closure dependency;
- package publication inventory and dependency graph;
- intended public/private disposition for every workspace package;
- package metadata changes;
- `binstall` disposition;
- final manual release sequence;
- results of `scripts/verify.sh full`;
- `cargo package` and package-list results for intended packages;
- `cargo publish --dry-run` results or precise registry limitation;
- workflow search proving no automated release authority remains;
- documentation search proving tag-triggered release claims are removed;
- explicit statement that no actual publish occurred unless separately authorized;
- residual manual operations and risks.

## 16. Handoff notes

Do not start by deleting the workflow. First establish the package graph and manual dry-run path so the repository does not lose its only documented release procedure before a replacement exists.

Do not assume all ten workspace members should be crates.io packages. Internal decomposition and public distribution are separate decisions.

Do not hide immutable-version handling behind vague language such as “retry the release.” A crates.io version that exists cannot be replaced. The guide must force a new version where correction is required.

Keep the final process manual and boring. A clear checklist is the desired architecture.
