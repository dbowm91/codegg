# Development Verification and Release Milestone 005 — Green Verification and crates.io Correctness Closure

Status: implemented

Repository baseline:

- production/documentation baseline: `942593852057dbd0914066a609e02ca57a016abf`
- corrective planning addendum: `ab3d75130b2d4281de7a122adaa769a0209ead98`

Source roadmaps:

- `plans/subsystems/development-verification-release-correctness-closure-addendum.md`
- `plans/subsystems/development-verification-release-roadmap.md`

Historical implementation plans corrected by this milestone:

- `plans/implementation/development-verification-release/001-routine-ci-contraction.md`
- `plans/implementation/development-verification-release/002-local-verification-contract.md`
- `plans/implementation/development-verification-release/003-manual-crates-io-release-ownership.md`
- `plans/implementation/development-verification-release/004-integration-evidence-cleanup-and-closure.md`

Historical closure records whose strict claims are superseded by this milestone:

- `plans/closure/development-verification-release/001-status.md`
- `plans/closure/development-verification-release/002-status.md`
- `plans/closure/development-verification-release/003-status.md`
- `plans/closure/development-verification-release/004-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#3-non-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- None. This milestone restores the previously approved verification and manual-release policy. It must stop rather than introduce hosted publication authority, a new canonical build system, or package renaming without approval.

Primary class: invariant

Secondary class: capability closure

## 1. Objective

Preserve the reduced one-job CI and local-first verification architecture while making every canonical verification path genuinely pass and restoring the maintainer-operated crates.io release capability required by the roadmap.

This milestone has four inseparable closure outcomes:

1. the sole GitHub Actions `verify` job is green-capable and has one observed successful run;
2. `scripts/verify.sh quick` and `scripts/verify.sh full` both exit zero under one documented resource contract;
3. scheduler-owned Bash/Python execution remains authoritative and its current failing test is repaired without bypasses;
4. the intended CodeGG crates.io package graph is publishable through a documented manual sequence, with successful package evidence and honest dry-run evidence.

The milestone is not complete if only documentation or status files change. It is not complete if failures are labeled pre-existing and left in required gates. It is not complete if crates.io publication is disabled to avoid packaging work.

## 2. Why this milestone is ready

The structural simplification is already landed:

- `.github/workflows/ci.yml` contains one bounded read-only job;
- `.github/workflows/release.yml` is absent;
- `.github/workflows/lsp-real-server.yml` is absent;
- release artifact aggregation and LSP compatibility aggregation are absent;
- optional test sources and local commands remain;
- `scripts/verify.sh` is the documented local entry point.

The remaining defects are concrete and reproducible:

- `scripts/check-tokio-test-flavors.py` reports 1,062 historical violations and exits 1;
- `scripts/verify.sh quick` therefore exits 1;
- `tool::bash::tests::active_mode_python_command_routes` fails because scheduler admission is unavailable in the test harness;
- the workspace test command therefore exits 1;
- local full verification omits the `RUST_MIN_STACK=33554432` bound already required in hosted CI;
- all workspace packages are marked `publish = false`, contrary to the manual crates.io release objective;
- active planning state claims closure despite those findings.

No new runtime architecture decision is required. The scheduler authority invariant is already established; the test must conform to it. The release owner is already established as the maintainer; package metadata must conform to it.

## 3. Current implementation evidence

### 3.1 Routine CI

`.github/workflows/ci.yml` currently has the desired topology:

- trigger: pull requests and pushes to `main`;
- one `verify` job;
- one Ubuntu runner;
- read-only `contents: read` permission;
- no matrix, `needs`, artifact upload, release command, language-server installation, WASM target installation, audit installation, or cross-target build;
- `CARGO_BUILD_JOBS=1`;
- `RUST_MIN_STACK=33554432`;
- `--test-threads=1` on broad tests.

The topology must be retained.

The job is not green-capable because:

```bash
python3 scripts/check-tokio-test-flavors.py
```

returns 1 against the accepted baseline, and the later workspace test command contains a known failing test.

### 3.2 Tokio flavor guard

`scripts/check-tokio-test-flavors.py` currently:

- detects bare `#[tokio::test]` attributes;
- has an empty default allowlist;
- treats every detected bare test as a current violation;
- exits 1 whenever any violation exists;
- describes itself as a regression guard, but has no historical baseline model.

The historical suite contains approximately 1,062 bare annotations. Bulk conversion is not required for this closure and would create a large unrelated test-runtime change.

### 3.3 Local verification

`scripts/verify.sh` currently defines:

```text
quick = format + generated-agent checks + Tokio guard + core-boundary guard + workspace check
full  = quick + Clippy + workspace tests + production-feature compile check
```

The script:

- correctly uses `set -euo pipefail`;
- correctly resolves the repository root from its own path;
- correctly sets `CARGO_BUILD_JOBS=1` for broad commands;
- correctly uses `--test-threads=1` for the full workspace test;
- does not export `RUST_MIN_STACK=33554432`;
- cannot currently complete quick or full successfully.

### 3.4 Scheduler-dependent test

The failing test is:

```text
tool::bash::tests::active_mode_python_command_routes
```

The existing evidence says it enters a Python execution path requiring scheduler admission while the default test harness has no enabled scheduler.

The production invariant is that command and Python execution remain scheduler-owned. A fix that directly executes Python, disables the scheduler check, catches/ignores the panic, marks the test ignored, or removes the test is invalid.

### 3.5 Package and release state

The root package and all workspace crates currently have `publish = false`. `RELEASING.md` says CodeGG is not published to crates.io and documents only manual binary builds plus an optional GitHub Release.

The approved policy is instead:

- release cadence and version choice are manual;
- crates.io publication is performed by the maintainer outside GitHub CI;
- GitHub tags/releases are optional manual follow-up operations;
- GitHub Actions has no publication authority.

The root `codegg` package depends on multiple internal workspace crates. The implementation must derive the actual transitive registry package graph rather than assuming that only the root manifest needs changes.

## 4. Invariants that must not regress

### CI and verification invariants

- Exactly one routine GitHub Actions job remains.
- Routine CI remains read-only and non-release.
- No matrix, artifact upload, release-mode cross-build, audit install, real-server install, example matrix, duplicated plugin test lane, or scheduled compatibility workflow is reintroduced.
- Required verification commands must exit nonzero on actual new regressions.
- Required verification commands must exit zero on the accepted implementation baseline.
- No `continue-on-error`, `|| true`, blanket exclusion, ignored test, or unconditional-success wrapper may conceal a required failure.
- `CARGO_BUILD_JOBS=1`, `--test-threads=1`, and `RUST_MIN_STACK=33554432` remain explicit for the broad test contract unless a measured replacement is documented and approved.
- CI and local verification must not claim different resource semantics for the same workspace test command.

### Tokio regression invariants

- New bare `#[tokio::test]` annotations fail verification.
- Historical bare tests are explicit and reviewable, not hidden by ignoring whole directories or returning success unconditionally.
- The baseline is keyed by stable test identity, not line number.
- Baseline entries cannot use wildcards, directory-level suppression, or substring patterns that permit unrelated new tests.
- Removing or converting a historical violation must permit the baseline to shrink.

### Scheduler ownership invariants

- Bash/Python execution remains scheduler-admitted.
- Tests must use the same ownership boundary as production when they claim to execute a command.
- Scheduler unavailability must produce a typed error/refusal, not a panic.
- No test-only direct spawn path may become reachable from production code.
- A routing-only test must not accidentally claim execution success.

### Release invariants

- GitHub Actions must not publish crates, create tags, create GitHub Releases, upload release assets, choose versions, or hold crates.io credentials.
- At least the intended installable CodeGG package must be configured for crates.io publication.
- Every transitive non-dev workspace dependency required by an intended publishable package must have a registry-resolvable version relationship.
- Private or fixture-only packages must remain explicitly non-publishable.
- Package names, versions, ownership, and dependency order must be derived and documented; they must not be guessed.
- No actual `cargo publish` occurs in this milestone.
- A missing crates.io package name or ownership grant is a blocker, not evidence that crates.io support should be disabled.
- An immutable published version is never replaced or retried as mutable state.

### Planning and evidence invariants

- Historical closure records remain preserved as historical evidence.
- Active planning state must acknowledge that strict closure transferred to M005.
- The implementation agent must not create `plans/closure/development-verification-release/005-status.md`.
- M005 closure must be written by a separate reviewer after implementation lands.
- A command returning nonzero cannot be recorded as `pass`.

## 5. Scope

### In scope

- `.github/workflows/ci.yml` only where needed to keep hosted and local verification aligned and green;
- `scripts/check-tokio-test-flavors.py`;
- a narrow checked-in Tokio flavor baseline file and focused script tests;
- `scripts/verify.sh`;
- focused verification-script tests or static checks that prevent resource-contract drift;
- the failing Bash/Python routing test and the smallest production/test-fixture changes needed to preserve scheduler ownership;
- a negative scheduler-unavailable test;
- root and workspace `Cargo.toml` publication metadata;
- path-plus-version dependency declarations for the intended publishable graph;
- package README/description/license/repository metadata required by Cargo/crates.io;
- `RELEASING.md`;
- active references in `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, and `architecture/testing.md` where they currently conflict;
- the corrective addendum, M005 implementation status, and registry state after implementation;
- successful local and hosted verification evidence.

### Explicitly out of scope

- converting all historical bare Tokio tests;
- removing the Tokio flavor guard;
- reducing required correctness coverage solely to make CI pass;
- ignoring or deleting the scheduler-dependent test;
- allowing a scheduler bypass;
- broad refactoring of Bash, Python, scheduler, or Tool Programs execution;
- restoring the old multi-job CI pipeline;
- reintroducing release or LSP workflows;
- adding a release framework or task runner;
- actual crates.io publication;
- selecting the next version;
- package renaming without explicit maintainer approval;
- source signing, SBOMs, attestations, notarization, Homebrew, AUR, Debian/RPM, containers, or installers;
- unrelated Clippy or test cleanup;
- rewriting historical closure records.

## 6. Required production changes

### Core/domain

No new product domain is expected. The only production-facing code change should be the smallest correction needed for the Bash/Python test to observe scheduler-owned behavior without panic.

If investigation shows production code currently panics when scheduler admission is unavailable, replace that panic with the existing typed command/tool/scheduler error boundary. Do not introduce a generic string error when a suitable typed error already exists.

### Storage and migrations

None expected.

Do not add migrations or durable state for verification or release policy.

### Protocol and DTOs

None expected.

If the scheduler-unavailable path crosses a protocol boundary and currently has no typed representation, stop and determine whether an existing error envelope can represent it. Do not add a protocol variant merely for a unit test without reviewing production behavior.

### Runtime and concurrency

The broad verification resource contract is:

```text
CARGO_BUILD_JOBS=1
RUST_MIN_STACK=33554432
cargo test ... -- --test-threads=1
```

`scripts/verify.sh full` must export or apply all three components. CI must retain equivalent values.

Do not increase test threads to compensate for runtime. Do not remove the stack bound until the underlying stack issue is independently fixed and measured.

### Frontend or operator surface

`RELEASING.md` must become the authoritative manual crates.io procedure. It must distinguish:

- reversible validation commands;
- commands that inspect package contents;
- registry ownership/name checks;
- dry-run commands;
- irreversible publication commands that are documented but not executed;
- dependency index propagation checks;
- optional manual tag/GitHub Release steps.

### Security and authorization

- No Cargo token, registry credential, GitHub token, or account identifier is committed.
- No workflow permission is widened.
- Fork pull requests cannot execute publication.
- The release guide must use maintainer-local Cargo credentials and must not echo them.
- The scheduler test fix must not weaken execution policy.

### Documentation and static guards

Active documentation must say:

- routine CI is one bounded job;
- quick and full verification are expected to pass;
- historical Tokio tests are baseline-managed while new bare tests fail;
- full verification uses the complete resource environment;
- crates.io is the primary manual package publication path;
- GitHub releases are optional manual binary distribution, not the replacement for crates.io;
- no hosted release automation exists.

Historical closure records should remain unchanged. The corrective addendum owns their post-review disposition.

## 7. Ordered work packages

### Work package A — Reproduce and freeze the current failures

Intent:

Establish exact failing behavior before modifying guards, tests, or metadata.

Required actions:

1. Run and record:

```bash
python3 scripts/check-tokio-test-flavors.py
scripts/verify.sh quick
RUST_MIN_STACK=33554432 CARGO_BUILD_JOBS=1 \
  cargo test --workspace --locked -- --test-threads=1
cargo test -p codegg --lib active_mode_python_command_routes -- --nocapture
```

2. Record:

- exact exit code;
- exact violation count from the Tokio guard;
- exact test binary and panic/error for the Bash/Python test;
- whether any additional workspace test fails;
- whether the stack bound is still required.

3. Inspect the current workflow and script command inventories and save the before-state in implementation notes.

Acceptance evidence:

- the implementation report contains exact commands and results;
- no finding is assumed solely from the historical closure records;
- any changed baseline is incorporated into the later work packages.

### Work package B — Make the Tokio flavor check a real baseline-aware regression guard

Intent:

Allow the accepted historical suite to pass while continuing to reject every newly introduced bare Tokio test.

Required design:

1. Add a checked-in baseline, preferably:

```text
scripts/tokio-test-flavor-baseline.txt
```

2. Baseline entries must identify tests using a stable form such as:

```text
relative/path/to/file.rs::test_function_name
```

3. The parser must associate a bare `#[tokio::test]` attribute with the following test function. It must handle the attribute/function layouts currently used by the repository, including intervening `#[cfg(...)]` or other attributes where present.

4. The default guard behavior must compute:

```text
current_bare_identities
baseline_identities
new_violations   = current - baseline
stale_baseline   = baseline - current
```

5. Required exit semantics:

- exit 0 only when `new_violations` is empty and `stale_baseline` is empty;
- exit 1 when a new bare test exists;
- exit 1 when stale baseline entries exist, forcing intentional baseline reduction in the same change;
- exit 1 for malformed baseline entries, duplicate entries, wildcard entries, unreadable files, or ambiguous/unidentified bare attributes.

6. Add a diagnostic mode such as `--emit-current` that prints sorted stable identities. If an update mode is added, it must be explicit and documented; normal CI must never rewrite the baseline.

7. Extend `--self-test` or add focused Python tests covering:

- explicit `current_thread` passes;
- explicit bounded `multi_thread` passes;
- historical identity in baseline passes;
- new bare identity fails;
- stale baseline identity fails;
- duplicate baseline entry fails;
- wildcard/directory suppression fails;
- multiline/cfg/attribute layouts map to the correct function;
- malformed or missing function after a bare attribute fails closed.

8. Generate the initial baseline from the current accepted repository state and review it for scope. Do not add `src/`, `tests/`, or another directory as one blanket exemption.

Example expected behavior:

```text
# accepted historical test
scripts/tokio-test-flavor-baseline.txt contains:
tests/session_crud.rs::legacy_session_round_trip

# same repository state
python3 scripts/check-tokio-test-flavors.py
-> exit 0

# new test added without explicit flavor
src/new_module.rs::new_async_test
-> exit 1 and print NEW violation

# historical test converted to current_thread but baseline not updated
-> exit 1 and print STALE baseline entry
```

Acceptance evidence:

- guard self-tests pass;
- repository guard exits 0;
- a temporary fixture adding a new bare test exits 1;
- a temporary fixture removing a baseline violation without updating the baseline exits 1;
- no broad ignore mechanism exists.

### Work package C — Repair the scheduler-owned Bash/Python test and negative path

Intent:

Make the current test truthful and passing while preserving canonical scheduler authority.

Required investigation:

Determine whether `active_mode_python_command_routes` is intended to prove:

1. command classification/routing only; or
2. end-to-end execution after scheduler admission.

Required implementation rules:

#### If the test is routing-only

- stop before execution;
- assert the typed command intent/plan selects the Python path;
- assert the produced execution request retains scheduler ownership metadata;
- rename or split the test so its name does not claim execution;
- add a separate admitted execution test only if existing coverage does not already prove it.

#### If the test is execution-bearing

- construct or reuse the smallest production-shaped scheduler/admission fixture;
- enable/start the scheduler through its normal test API;
- submit through the canonical broker/dispatcher;
- wait using deterministic completion primitives, not arbitrary sleeps;
- cleanly shut down the fixture.

#### Required negative test

Add a focused test proving that scheduler unavailability:

- returns the existing typed scheduler/admission/tool error;
- does not panic;
- does not spawn Python or a shell directly;
- leaves no orphan process;
- does not mutate job/attempt state as if admitted.

Invalid fixes:

- `#[ignore]`;
- `#[should_panic]`;
- catching and discarding the panic;
- removing the assertion;
- bypassing scheduler admission in tests;
- adding a production fallback that directly spawns when the scheduler is unavailable;
- excluding the test from workspace verification.

Acceptance evidence:

```bash
cargo test -p codegg --lib active_mode_python_command_routes -- --nocapture
cargo test -p codegg --lib scheduler_unavailable -- --nocapture
```

Both commands exit 0, and the negative case proves typed refusal without panic or spawn.

### Work package D — Unify hosted and local resource contracts

Intent:

Ensure the same broad workspace tests have the same stack/build/test limits locally and in CI.

Required changes:

1. In `scripts/verify.sh`, establish defaults without overriding an explicit maintainer choice:

```bash
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export RUST_MIN_STACK="${RUST_MIN_STACK:-33554432}"
```

2. Continue passing:

```text
-- --test-threads=1
```

for broad workspace tests.

3. Print the effective broad-test environment before the test command so failure logs are actionable.

4. Retain equivalent CI environment values.

5. Add a focused shell/static test or self-test that proves:

- default values are applied;
- caller-provided values are not unexpectedly overwritten, except `--test-threads=1` remains canonical for the broad mode;
- invoking from a nested directory still resolves the root;
- unknown mode still fails;
- quick/full propagate failures.

6. Do not add another task runner or configuration language.

Acceptance evidence:

- shell syntax test passes;
- verification-script focused tests pass;
- full mode output records the stack/build/test limits;
- CI and local documentation show the same values.

### Work package E — Make quick and full verification genuinely pass

Intent:

Close the canonical developer/maintainer contract after B–D land.

Required sequence:

```bash
bash -n scripts/verify.sh
python3 scripts/check-tokio-test-flavors.py --self-test
python3 scripts/check-tokio-test-flavors.py
scripts/verify.sh quick
scripts/verify.sh full
```

Requirements:

- all commands exit 0;
- quick must not skip its guard;
- full must run quick, Clippy, the bounded workspace tests, and the production-feature compile check;
- full must not silently omit failing workspace tests;
- no optional real language server is required;
- no release publication occurs;
- no command uses `|| true` or equivalent failure suppression.

If another workspace failure appears after the known failure is fixed, correct it only when it is within the verification/test-harness boundary and narrow enough for this milestone. Otherwise stop and register a precise blocker rather than excluding it.

Acceptance evidence:

- complete command logs or concise exact summaries with exit codes;
- total test counts and any ignored-test count;
- confirmation that no newly ignored test was added to obtain success.

### Work package F — Restore the intended crates.io package graph

Intent:

Replace the accidental all-private policy with an explicit, technically publishable package graph for CodeGG.

Required package policy:

- the root installable `codegg` package is intended for crates.io publication unless crates.io name/ownership evidence proves a blocker;
- every transitive non-dev workspace dependency needed by the packaged root must be classified;
- required internal crates must be publishable with complete metadata and path-plus-version dependencies unless an already-reviewed architecture makes them unnecessary to the packaged root;
- test fixtures, examples, or genuinely private-only packages may remain `publish = false`;
- do not make every crate public merely for convenience without confirming it is in the required graph.

Required inventory:

Run:

```bash
cargo metadata --format-version 1 --no-deps
```

Create a table in implementation/closure evidence containing:

- package name;
- manifest path;
- current version;
- intended publish policy;
- reason for policy;
- direct internal dependencies;
- topological publication position;
- crates.io name availability/ownership status;
- required metadata gaps.

Required metadata changes for every intended publishable crate:

- remove or correct `publish = false`;
- package name and version;
- edition and `rust-version` where appropriate;
- description;
- license;
- repository;
- homepage only when accurate;
- README path that is included in the package;
- relevant keywords/categories where useful but not at the expense of correctness;
- path-plus-version form for publishable internal dependencies, for example:

```toml
codegg-core = { version = "=0.1.0", path = "crates/codegg-core" }
```

Use the actual version and an intentionally selected semver requirement. Exact requirements may be preferable for a tightly coupled initial workspace release; document the choice.

Dev-dependencies and build-dependencies must also be reviewed. A packaged crate must not rely on an unavailable path-only package during verification.

Package-content requirements:

For each intended publishable package:

```bash
cargo package -p <package> --list
cargo package -p <package>
```

Inspect for:

- missing source or migration files;
- missing README/license;
- generated files required for compilation;
- accidental large fixtures, target artifacts, local databases, logs, secrets, or planning evidence;
- path dependencies that normalize without a registry version;
- build scripts that assume repository-only paths.

Dry-run requirements:

```bash
cargo publish --dry-run -p <package>
```

Interpretation must be honest:

- rejection because `publish = false`, missing metadata, invalid package contents, path-only dependency, build failure, test failure, or version mismatch is a failure;
- for a dependent package whose required internal dependency version has never been actually published, the exact crates.io registry-absence error may be recorded as an expected pre-publication sequencing constraint only after:
  - every leaf package dry-run passes;
  - every package build/package verification passes;
  - the normalized packaged manifest contains the correct registry version requirement;
  - registry name/ownership has been checked;
  - `RELEASING.md` places the dependency earlier in the actual publish order;
- do not call a dependent dry-run `pass` when it returned nonzero; classify it as `blocked until dependency publication` and explain why actual publication is outside this milestone.

Registry ownership stop condition:

If `codegg` or any required dependency crate name is unavailable or not controlled by the maintainer:

- stop;
- record the exact package name and evidence;
- do not rename the package;
- do not revert to `publish = false` for the entire graph;
- leave M005 blocked pending maintainer decision.

No actual `cargo publish` may be run.

Acceptance evidence:

- publication inventory;
- clean package lists;
- successful `cargo package` for every intended package;
- successful dry-runs where registry sequencing permits;
- exact nonzero registry-sequencing results where actual prior publication is required;
- no package fails for local metadata or package correctness reasons.

### Work package G — Rewrite the manual release procedure around crates.io

Intent:

Make `RELEASING.md` implement the approved maintainer-operated release path rather than a binary-only replacement.

Required structure:

#### 1. Scope and ownership

State clearly:

- crates.io publication is manual;
- version and cadence are maintainer decisions;
- GitHub Actions does not publish;
- GitHub tags and binary releases are optional separate actions.

#### 2. Clean-tree and account preflight

Include:

```bash
git switch main
git pull --ff-only
git status --short
cargo login --help  # documentation only; do not expose token
cargo owner --list <package>
```

Use the appropriate ownership inspection commands without printing credentials.

#### 3. Version and changelog preparation

Document:

- all tightly coupled package versions that must change together;
- dependency requirement updates;
- immutable version rule;
- clean-tree check after edits.

#### 4. Verification

Use only:

```bash
scripts/verify.sh full
```

plus change-specific optional checks. Do not duplicate a divergent full command list.

#### 5. Package inspection and dry-run

List the actual packages in topological order with concrete commands.

#### 6. Irreversible publication

Clearly label but do not execute:

```bash
cargo publish -p <leaf-package>
# verify registry/index availability
cargo publish -p <dependent-package>
```

Document bounded index propagation checks before publishing dependents.

#### 7. Partial failure and immutability

State:

- successful versions cannot be overwritten;
- fix and bump the affected package/version as required;
- yanking is not deletion;
- do not blindly rerun the same version;
- record which packages were successfully published before continuing.

#### 8. Tags and optional GitHub binary release

Keep these manual and separate. Do not imply that a tag triggers automation.

#### 9. Installation verification

Document the expected end-user command after an actual release, for example:

```bash
cargo install codegg --version <VERSION>
```

Retain `cargo install --path .` only as source/development installation.

Acceptance evidence:

- another agent can follow the reversible steps without reading historical plans;
- every package command names a real package;
- crates.io is not described as unsupported;
- optional binary release steps do not replace crates.io publication.

### Work package H — Hosted verification and planning reconciliation

Intent:

Prove the reduced hosted gate works and restore evidence-faithful planning state.

Required workflow checks:

```bash
find .github/workflows -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) -print | sort
rg --line-number --glob '.github/workflows/*.{yml,yaml}' \
  'cargo publish|gh release create|CARGO_REGISTRY_TOKEN|contents: write|packages: write|id-token: write|upload-artifact|download-artifact|schedule:' \
  .github/workflows
```

Expected result:

- only the bounded routine workflow remains;
- no release authority or artifact machinery is present.

Required hosted evidence:

- push the implementation revision through the normal repository workflow;
- obtain one successful `verify` job attached to the implementation commit or PR head;
- record workflow run ID, commit SHA, conclusion, and job/step summary in the later closure record;
- do not create a replacement evidence workflow.

If branch protection still names removed jobs, report the repository-setting mismatch. Updating branch protection is an operational maintainer action and must not be simulated in YAML.

Required planning changes after implementation lands:

- mark M005 implementation plan `implemented` or `closing`, not `closed`;
- mark the corrective addendum `closing`;
- update `plans/registry.md` to move M005 from ready to closing;
- preserve `plans/closure/development-verification-release/005-status.md` as absent;
- note that a separate reviewer must create the closure record.

Acceptance evidence:

- successful hosted run;
- workflow-policy searches;
- accurate registry/addendum state;
- no M005 closure record created by the implementation agent.

## 8. Failure, cancellation, restart, and contention semantics

### Verification commands

- Fail fast on the first failing command.
- Preserve the underlying exit code.
- Print the command and effective resource environment.
- Do not continue into packaging after full verification fails.
- Interrupted verification is restartable from the beginning; no persistent verification state is required.

### Tokio baseline

- Concurrent edits to the baseline and tests must be reviewed together.
- A merge that adds a new bare test without its explicit flavor fails.
- A merge that removes a historical violation without shrinking the baseline fails as stale.
- Baseline generation must be deterministic and sorted to minimize merge conflict.

### Scheduler fixture

- Fixture startup failure is a test failure.
- Cancellation must stop admitted work and clean process trees through existing scheduler ownership.
- Test shutdown must not leave global scheduler state that contaminates later tests.
- Parallel execution must not rely on fixed ports or process-global mutable configuration unless the test is explicitly serialized.

### Package preparation

- Package generation is restartable.
- Actual publication is non-transactional and out of scope.
- Registry lookup failure must be distinguished from package correctness failure.
- Two maintainers must not publish the same release concurrently; document serialization in `RELEASING.md`.

## 9. Compatibility and migration

### CI compatibility

The check name remains `verify` unless a compelling repository constraint requires otherwise. Do not restore old check names.

Branch protection may require a settings update from removed historical job names to `verify`. Report this explicitly if observable.

### Test compatibility

The Tokio baseline is a compatibility bridge for historical tests, not a permanent endorsement. New tests must use explicit flavors. Existing tests may be converted incrementally with matching baseline reduction.

### Package compatibility

Changing `publish = false` and adding registry versions must not change Rust APIs or runtime behavior.

Path-plus-version dependencies must continue to use local paths during workspace development and registry versions in packaged manifests.

### Installation compatibility

- source installation remains `cargo install --path .`;
- after an actual crates.io release, registry installation becomes `cargo install codegg`;
- optional GitHub binaries remain a separate distribution path if maintainers choose to produce them manually.

## 10. Required tests

### Focused unit tests

Tokio guard:

- parser identifies stable test function names;
- explicit flavors are excluded;
- new violation detection;
- stale baseline detection;
- duplicate/malformed/wildcard baseline rejection;
- deterministic sorted output.

Verification script:

- help and unknown mode;
- root resolution from nested directory;
- default environment application;
- explicit environment preservation;
- failure propagation.

Bash/Python routing:

- active-mode Python routing selects the correct typed route;
- admitted execution passes if execution is part of the contract;
- scheduler-unavailable path returns typed failure without panic.

### Integration tests

- full `scripts/verify.sh quick` execution;
- full `scripts/verify.sh full` execution;
- production-feature compile check;
- package generation from clean tree for each intended package;
- package archive compilation/verification through Cargo.

### Restart and recovery tests

No new durable runtime recovery tests are expected.

For the scheduler fixture, ensure repeated test setup/teardown does not retain global state or orphan processes.

### Contention and cancellation tests

Only required if the repaired scheduler test starts real admitted work. Reuse existing fixture-level cancellation coverage where adequate; add one focused cleanup assertion if the current failure path can orphan work.

### Security and negative tests

- new bare Tokio test fails the guard;
- stale baseline fails the guard;
- scheduler unavailable does not spawn;
- workflow policy search finds no publication authority;
- intended package cannot be accidentally selected by a workflow;
- private packages remain rejected for publication intentionally.

### Migration and compatibility tests

- local workspace path dependencies still resolve;
- packaged manifests contain registry versions;
- package archives do not rely on repository-relative files omitted from the archive;
- source install still works from a clean checkout:

```bash
cargo install --path . --locked --force
```

Use a temporary install root when practical to avoid changing the maintainer's active binary.

## 11. Required verification commands

Run narrow checks first, then broad verification, then packaging.

```bash
# Script syntax and focused guard tests
bash -n scripts/verify.sh
python3 scripts/check-tokio-test-flavors.py --self-test
python3 scripts/check-tokio-test-flavors.py

# Prove new violations fail using a temporary fixture or focused test harness
python3 -m unittest discover scripts/tests -p '*tokio*' -v

# Scheduler-owned routing and negative behavior
cargo test -p codegg --lib active_mode_python_command_routes -- --nocapture
cargo test -p codegg --lib scheduler_unavailable -- --nocapture

# Canonical local contract
scripts/verify.sh quick
scripts/verify.sh full

# Workflow policy
find .github/workflows -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) -print | sort
rg --line-number --glob '.github/workflows/*.{yml,yaml}' \
  'cargo publish|gh release create|CARGO_REGISTRY_TOKEN|contents: write|packages: write|id-token: write|upload-artifact|download-artifact|schedule:' \
  .github/workflows

# Package graph inventory
cargo metadata --format-version 1 --no-deps

# Repeat for every intended publishable package in topological order
cargo package -p <package> --list
cargo package -p <package>
cargo publish --dry-run -p <package>

# Source-install compatibility, preferably with a temporary CARGO_INSTALL_ROOT
CARGO_INSTALL_ROOT="$(mktemp -d)" cargo install --path . --locked
```

The implementation agent must replace placeholders in its execution report with the actual package order.

Do not run actual `cargo publish`.

## 12. Documentation updates

Update as required:

- `RELEASING.md` — manual crates.io procedure and package order;
- `architecture/testing.md` — baseline-aware guard and unified resource contract;
- `AGENTS.md` — canonical quick/full commands and expected success;
- `CONTRIBUTING.md` — same canonical references;
- `README.md` — installation/distribution distinction where appropriate;
- package-level READMEs or manifests required for packaging;
- `plans/subsystems/development-verification-release-correctness-closure-addendum.md` — status to closing after implementation;
- `plans/registry.md` — M005 to closing after implementation;
- this implementation plan — status to closing/implemented after implementation.

Do not modify historical closure records 001–004.

## 13. Acceptance criteria

M005 implementation is ready for independent closure only when every statement below is true.

### CI structure

- There is exactly one routine GitHub Actions job.
- It has read-only permissions.
- It has no matrix, release, artifact, audit-install, external-LSP, example-matrix, duplicated plugin, or cross-target work.
- Every step in one observed hosted run succeeds.

### Tokio guard

- The accepted repository baseline exits 0.
- A newly added bare Tokio test exits 1.
- A stale baseline entry exits 1.
- Baseline entries identify individual tests, not directories or wildcards.
- Guard self-tests pass.

### Scheduler test

- `active_mode_python_command_routes` passes.
- Scheduler unavailability returns typed failure without panic.
- No scheduler bypass or direct-spawn fallback exists.
- No relevant test is ignored or excluded.

### Local verification

- `scripts/verify.sh quick` exits 0.
- `scripts/verify.sh full` exits 0.
- Full verification uses `CARGO_BUILD_JOBS=1`, `RUST_MIN_STACK=33554432`, and `--test-threads=1` by default.
- Production-feature compilation passes.
- Failure propagation remains fail-closed.

### crates.io release capability

- The intended installable CodeGG package is not `publish = false`.
- Every required transitive workspace dependency has an explicit and justified publication policy.
- Intended publishable packages have complete metadata and registry-compatible dependency declarations.
- `cargo package` passes for every intended publishable package.
- Leaf-package `cargo publish --dry-run` commands pass.
- Any dependent-package dry-run blocked only by intentionally unpublished prerequisite versions is recorded as blocked, not passed, with correct package sequence evidence.
- No intended package fails due to `publish = false`, path-only dependency, missing metadata, missing archive content, compile failure, or test failure.
- Registry name/ownership conflicts, if any, block closure and are reported exactly.
- No actual publication occurs.

### Release ownership and documentation

- No workflow can publish or create a release.
- `RELEASING.md` describes manual crates.io publication as the primary package release path.
- Version immutability, dependency order, index propagation, partial failure, yanking, tags, and optional GitHub releases are documented.
- Active documentation matches the actual commands and package policy.

### Planning state

- M005 is marked closing after implementation.
- The corrective addendum is marked closing.
- The registry identifies M005 as active/closing.
- `plans/closure/development-verification-release/005-status.md` is absent until an independent reviewer creates it.

## 14. Stop conditions

The implementation agent must stop and report rather than improvise when:

- a required crates.io package name is unavailable or not controlled by the maintainer;
- satisfying crates.io packaging would require renaming a package;
- satisfying packaging would require a broad public API redesign;
- the root package cannot be made publishable without a material architecture decision;
- a verification failure is outside this subsystem and cannot be corrected narrowly;
- the scheduler test cannot be made production-shaped without changing scheduler ownership;
- fixing scheduler unavailability would require a protocol or storage redesign;
- hosted Actions evidence cannot be obtained after implementation;
- branch protection or repository settings prevent the new `verify` check from serving as the required gate and the agent cannot access settings;
- a change would reintroduce deleted CI/release/evidence machinery;
- actual crates.io publication would be required to proceed.

A stop-condition report must name the exact file, command, package, or repository setting and preserve all completed narrow work without claiming closure.

## 15. Closure evidence required

A separate reviewer must create `plans/closure/development-verification-release/005-status.md` containing:

1. reviewed implementation commit SHA;
2. diff summary proving the one-job topology remains;
3. exact Tokio guard baseline format and self-test results;
4. proof that current baseline passes, a new violation fails, and stale baseline fails;
5. focused scheduler test results and negative typed-error evidence;
6. `scripts/verify.sh quick` exact exit result;
7. `scripts/verify.sh full` exact exit result;
8. effective `CARGO_BUILD_JOBS`, `RUST_MIN_STACK`, and test-thread values;
9. workspace test totals and ignored-test delta;
10. GitHub Actions workflow run ID, commit SHA, conclusion, and step summary;
11. package inventory and dependency order;
12. package-list inspection results;
13. `cargo package` results for every intended package;
14. dry-run results classified accurately as pass, expected registry-sequencing blocker, or failure;
15. crates.io name/ownership evidence;
16. workflow release-authority search results;
17. source-install compatibility result;
18. active documentation reconciliation;
19. explicit confirmation that no actual package was published;
20. residual findings table.

Strict closure requires no unresolved high or medium finding. A missing hosted green run, failing quick/full command, or unresolved package ownership/name conflict prevents strict closure.

## 16. Handoff notes

- Preserve all user changes unrelated to this milestone.
- Do not recreate deleted workflows.
- Do not perform bulk Tokio test conversion.
- Prefer stable test identities over line-number baselines.
- Keep baseline output deterministic and sorted.
- The test suite is resource-heavy; run narrow tests before full verification.
- Use `CARGO_BUILD_JOBS=1`, `RUST_MIN_STACK=33554432`, and `--test-threads=1` for broad tests.
- Avoid fixed ports and process-global state in new scheduler fixtures.
- Do not run actual `cargo publish`.
- Do not assume crates.io package ownership from repository ownership; verify it.
- Do not mark M005 closed. Leave independent closure to a separate reviewer.
