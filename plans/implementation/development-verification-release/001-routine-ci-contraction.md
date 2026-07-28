# Development Verification and Release Milestone 001 — Routine CI Contraction

Status: ready for handoff

Repository baseline: `39d0720f9748cabc978ad9b0a3a32c31c6bc84d1` plus the planning-only registration series beginning at `4f5e0213b25aa4bce32b1d50abf37b8a48ef4493`

Source roadmap:

- `plans/subsystems/development-verification-release-roadmap.md#milestone-001--routine-ci-contraction`

Long-term requirements:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#3-non-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`

Applicable ADRs:

- None.

Primary class: infrastructure

## 1. Objective

Replace the current multi-job routine GitHub Actions pipeline with one bounded, non-duplicated verification job that answers the ordinary development question: does this revision format, satisfy required static source guards, compile, lint, and pass the default deterministic workspace test surface?

This milestone must materially reduce runner count, repeated dependency setup, broad feature activation, release-mode compilation, artifact production, and unrelated external-tool installation. It must not delete the underlying tests or optional verification capabilities owned by later milestones.

## 2. Why this milestone is ready

The maintainer has explicitly selected the operational policy:

- routine CI must be small and fast enough not to impede iteration;
- extensive verification belongs primarily in bounded local workflows;
- release cadence and publication are manual rather than GitHub-CI-owned;
- routine CI does not need release binaries or closure-evidence artifacts.

No production protocol, storage, scheduler, daemon, or frontend decision blocks this work. The current workflow and test commands are fully inspectable at the repository baseline.

Milestone 001 intentionally changes only routine CI. It leaves the separate release and LSP workflows in place until Milestones 003 and 004 so each ownership transfer has explicit replacement documentation and closure evidence.

## 3. Current implementation evidence

At the reviewed baseline, `.github/workflows/ci.yml` triggers for pushes and pull requests to both `dev` and `main` and contains these jobs:

- `agent-assets`;
- `fmt`;
- `check`;
- `clippy`;
- `test`;
- `plugin-focused`;
- `examples`;
- `audit`;
- `build-cross`, with four target entries.

The broad test job already executes:

```bash
cargo test --workspace --all-features -- --test-threads=14
```

The `plugin-focused` job then reruns plugin install, management, registry, and TUI command tests. Its unique contract is `scripts/check-core-boundary.sh`; the repeated plugin tests are diagnostic duplication rather than distinct coverage.

The `examples` job validates independent Rust/Python SDK examples and installs the WASM target. The `audit` job installs `cargo-audit` from source. The `build-cross` matrix performs release-mode builds and uploads binaries. None is required for ordinary pull-request feedback.

The routine workflow also repeatedly checks out the repository, installs Rust, restores caches, and recompiles overlapping dependency graphs in separate runners. Some jobs are serialized behind `needs`, further increasing wall-clock latency.

Active documentation calls a fourteen-thread full test run serial or conservative even though it allows fourteen concurrent tests per binary. The broader terminology correction belongs to Milestone 002; this milestone must stop introducing or preserving misleading comments in the workflow itself.

## 4. Invariants that must not regress

- Generated built-in agent definitions must remain checked against their canonical source.
- Bare `#[tokio::test]` regression prevention must remain active.
- The `codegg-core` dependency/import boundary must remain checked.
- Formatting failures must fail routine CI.
- Default-feature workspace compile failures must fail routine CI.
- Default-feature workspace Clippy warnings must fail routine CI.
- Default-feature workspace test failures must fail routine CI.
- Broad test execution must use explicit build and test concurrency bounds.
- Routine CI must not publish, tag, create a GitHub Release, or gain access to release credentials.
- Routine CI must not upload binaries, checksums, compatibility reports, or evidence manifests.
- Optional feature, plugin, example, LSP, audit, and cross-platform commands must remain present in the repository or documentation for later local/manual use.
- Historical closure documents must not be rewritten to pretend they used the new commands.

## 5. Scope

### In scope

- `.github/workflows/ci.yml` triggers, job graph, environment, and commands;
- removal of routine job duplication and artifact production;
- movement of essential static checks into one verification job;
- explicit `CARGO_BUILD_JOBS=1` and `--test-threads=1` for broad tests;
- default-feature rather than `--all-features` routine validation;
- pull-request and mainline-push trigger deduplication;
- concise comments explaining what routine CI intentionally does not cover;
- minimal active-document references required to prevent immediate command drift.

### Explicitly out of scope

- deleting `.github/workflows/release.yml`;
- deleting or redesigning `.github/workflows/lsp-real-server.yml`;
- creating `RELEASING.md`;
- changing Cargo package versions or publication metadata;
- deleting Nextest profiles or timing scripts;
- reorganizing production tests;
- changing Tokio runtime annotations or test database helpers;
- adding a new task runner, Makefile framework, release tool, or workflow generator;
- making all-features verification disappear from local development;
- changing branch-protection settings through repository administration unless the implementation agent has explicit access and the removed checks would otherwise block merging.

## 6. Required production changes

### Core/domain

No production Rust domain changes are expected.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

Rewrite `.github/workflows/ci.yml` around one job named `verify` or an equivalently stable name.

The workflow target is:

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  CARGO_BUILD_JOBS: 1

jobs:
  verify:
    runs-on: ubuntu-latest
```

The exact step order may vary to improve early failure, but it must include:

1. checkout;
2. stable Rust with `rustfmt` and `clippy`;
3. one Rust cache restore;
4. generated-agent schema/source checks;
5. Tokio test-flavor guard;
6. `codegg-core` boundary guard;
7. `cargo fmt --check --all`;
8. default-feature workspace check;
9. default-feature workspace Clippy with warnings denied;
10. default-feature workspace tests with one test thread.

Preferred commands:

```bash
python3 scripts/generate_builtin_agents.py --check
python3 scripts/check_builtin_agents.py
python3 scripts/check-tokio-test-flavors.py
./scripts/check-core-boundary.sh
cargo fmt --check --all
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
```

The implementation agent may adjust a command only when repository evidence shows the preferred form cannot run. Any adjustment must remain default-feature, bounded, and documented in the closure record. Do not restore `--all-features` as a convenience workaround.

The workflow must not contain:

- a strategy matrix;
- `needs` relationships;
- release-mode builds;
- `cross` installation;
- WASM target installation;
- Python SDK example execution;
- `cargo-audit` installation or execution;
- language-server installation;
- `actions/upload-artifact` or `actions/download-artifact`;
- duplicated focused plugin test invocations;
- generated checksums;
- GitHub Release commands.

Do not add path-filter logic in this milestone. One small unconditional job is easier to reason about than several conditionally skipped lanes.

### Frontend or operator surface

The visible operator surface is the GitHub checks page. It should display one required result with command-level steps that identify the failing category. Do not collapse every command into one opaque shell pipeline that stops without showing which category failed.

### Security and authorization

- Use no write permission unless GitHub requires an explicit read-only declaration.
- Prefer `permissions: contents: read` at workflow or job scope.
- Do not expose secrets.
- Do not grant packages, releases, attestations, id-token, pull-requests, or actions write permissions.
- Do not execute publication-capable commands.

### Documentation and static guards

Update only active guidance directly coupled to the workflow if necessary. At minimum, remove or correct any statement that routine CI still performs cross-target release builds, repeated plugin lanes, examples, or audit.

Milestone 002 owns the comprehensive documentation and canonical local command reconciliation. Avoid duplicating that work here.

## 7. Ordered work packages

### Work package A — Capture the current CI contract

Intent:

Identify which current routine steps provide unique correctness coverage and which are duplicate, release-specific, optional, or diagnostic-only.

Required changes:

- Record the current job names, triggers, matrices, artifact steps, and unique static guards.
- Confirm the plugin-focused tests are included by the broad workspace all-features test command.
- Confirm `scripts/check-core-boundary.sh` runs independently of the repeated plugin tests.
- Confirm generated-agent and Tokio guard scripts require no third-party Python packages or, if they do, retain the smallest necessary Python setup.
- Note any current branch-protection check names that may require external repository administration after the workflow change.

Acceptance evidence:

- A before/after job graph in the closure record.
- A table classifying each removed job as duplicate, optional, release-specific, or moved guard.
- No removed unique guard is unaccounted for.

### Work package B — Replace the workflow with one bounded job

Intent:

Make ordinary hosted feedback small, deterministic, and actionable.

Required changes:

- Replace the trigger block with pull requests plus pushes to `main`.
- Remove `dev` push validation to avoid duplicate push and pull-request runs for active branches.
- Add one `verify` job.
- Install one stable Rust toolchain with formatting and Clippy components.
- Restore one Cargo cache.
- Run the essential checks listed in Section 6.
- Set `CARGO_BUILD_JOBS=1` globally or on every broad Cargo command.
- Set `--test-threads=1` on the broad workspace test command.
- Use `--locked` when the committed lockfile supports it.

Acceptance evidence:

- Workflow contains exactly one job under `jobs:`.
- The job contains no matrix and no `needs`.
- The job contains no artifact actions or release build command.
- All required commands are visible as named steps or clearly separated run steps.

### Work package C — Remove hot-path optional work without deleting capability

Intent:

Stop invoking expensive optional verification on every ordinary change while preserving its implementation surface.

Required changes:

- Remove plugin-focused reruns from routine CI.
- Remove example SDK/WASM validation from routine CI.
- Remove dependency audit from routine CI.
- Remove cross-target release-mode builds and artifact uploads from routine CI.
- Do not delete their tests, examples, scripts, feature gates, or documentation in this milestone.
- Add one concise workflow comment or active-document note that optional checks are local/change-specific and will be canonically documented by Milestone 002.

Acceptance evidence:

- Repository search confirms the underlying plugin/example/audit/cross-build commands remain available outside routine CI.
- No test source file or example is deleted as part of this milestone.

### Work package D — Validate workflow behavior and reconcile immediate references

Intent:

Ensure the simplified workflow is syntactically valid and its command contract works from a clean checkout.

Required changes:

- Run every workflow command locally in workflow order where the environment permits.
- Inspect YAML indentation and trigger semantics.
- Update active CI documentation that would otherwise be immediately false.
- Report branch-protection follow-up if removed check names are still required and repository administration is unavailable.

Acceptance evidence:

- Exact command results with exit status.
- Final workflow excerpt or parsed job inventory.
- Explicit list of environment-dependent commands not run, if any.

## 8. Failure, cancellation, restart, and contention semantics

A command failure must fail the single job directly. Do not use `continue-on-error`, blanket `if: always()`, shell `|| true`, or aggregation logic that converts failure into a report artifact.

Cancellation should terminate the current runner and its Cargo subprocess tree through normal GitHub Actions behavior. Do not add custom retry loops or workflow-level reruns.

A new push to the same pull request may use workflow concurrency cancellation if and only if it is implemented with a small standard `concurrency` group and `cancel-in-progress: true`. This is optional. Do not introduce custom concurrency keys or scripts.

Cache failure must not be fatal; verification must still execute without a restored cache.

A network outage while installing the Rust toolchain or restoring dependencies remains a visible infrastructure failure. Do not duplicate jobs as defense in depth.

## 9. Compatibility and migration

The workflow check name will change if current branch protection expects individual `fmt`, `check`, `clippy`, `test`, or plugin jobs. The implementation agent must:

- inspect available repository settings if authorized;
- update required checks to the new single job if possible;
- otherwise report the exact old check names requiring maintainer removal.

No source, protocol, storage, or user-data migration is involved.

Direct pushes to `dev` will no longer trigger routine CI. Pull requests from `dev` or any other branch remain covered, and pushes to `main` remain covered.

Optional feature regressions are managed by Milestone 002's change-specific local contract. This milestone must not claim all-features or all-platform coverage.

## 10. Required tests

### Focused unit tests

No new Rust unit tests are required solely for workflow restructuring.

### Integration tests

Run the exact commands retained by the workflow from a clean or clean-equivalent checkout.

### Restart and recovery tests

Not applicable to production runtime. Confirm a cache miss does not alter workflow correctness by ensuring cache restore is not a required predecessor for commands.

### Contention and cancellation tests

Inspect optional GitHub workflow concurrency configuration if added. No custom contention test is required.

### Security and negative tests

Repository searches must confirm `.github/workflows/ci.yml` contains none of:

```text
cargo publish
gh release create
actions/upload-artifact
actions/download-artifact
permissions: contents: write
id-token: write
packages: write
strategy:
release --target
cargo install cargo-audit
rustup target add wasm32-unknown-unknown
```

The exact search may distinguish harmless comments from executable steps.

### Migration and compatibility tests

Confirm pull-request and `main` push triggers are present and `dev` push trigger is absent.

## 11. Required verification commands

Run narrow source guards first:

```bash
python3 scripts/generate_builtin_agents.py --check
python3 scripts/check_builtin_agents.py
python3 scripts/check-tokio-test-flavors.py
./scripts/check-core-boundary.sh
```

Run the intended routine commands with explicit bounds:

```bash
cargo fmt --check --all
CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
```

Inspect workflow structure and forbidden release/artifact content:

```bash
python3 - <<'PY'
from pathlib import Path
text = Path('.github/workflows/ci.yml').read_text()
required = ['pull_request:', 'branches: [main]', 'CARGO_BUILD_JOBS: 1']
for token in required:
    assert token in text, token
for token in [
    'actions/upload-artifact',
    'actions/download-artifact',
    'cargo publish',
    'gh release create',
    'cargo install cargo-audit',
    'rustup target add wasm32-unknown-unknown',
]:
    assert token not in text, token
print('routine CI structural checks passed')
PY
```

If YAML tooling is already available in the repository, use it to confirm the parsed job count. Do not add a new dependency solely for this check.

## 12. Documentation updates

Update as narrowly required:

- `.github/workflows/ci.yml` comments;
- `architecture/testing.md` CI-structure section if it would otherwise describe removed jobs as active;
- `AGENTS.md` or `CONTRIBUTING.md` only where they explicitly describe the old hosted workflow.

Add a clear note that broader command reconciliation is owned by Milestone 002. Do not preemptively add a release guide.

## 13. Acceptance criteria

- Pull requests produce one routine verification job.
- Pushes to `main` produce one routine verification job.
- Pushes to `dev` alone do not produce a duplicate routine run.
- The workflow contains no job matrix, job dependency chain, artifact upload, release-mode cross-build, example matrix, audit installation, or repeated plugin test lane.
- The retained job checks generated agents, Tokio test annotations, core boundary, formatting, default-feature compilation, default-feature strict linting, and default-feature workspace tests.
- Broad build concurrency is one and broad test concurrency is one.
- Workflow permissions are read-only.
- Every retained command is executed or an environment limitation is explicitly recorded.
- Underlying optional tests and examples remain in the repository.
- Branch-protection migration is completed or precisely reported as an external follow-up.
- No production source behavior changes.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- a retained required guard depends on a removed job's side effect and cannot run independently;
- default-feature workspace verification omits a production configuration that repository evidence shows is always shipped and must be routine-gated;
- the committed lockfile cannot support `--locked` without unrelated dependency updates;
- branch protection cannot be updated and the repository would become unmergeable after job-name removal;
- workflow syntax cannot express the required trigger without broadening scope;
- a proposed fix requires reorganizing the test suite, Cargo feature model, or production modules;
- the only way to pass is to suppress warnings, ignore tests, add retries, or reintroduce duplicated lanes;
- unrelated user changes are present in the same files and cannot be safely preserved.

## 15. Closure evidence required

The later closure record must contain:

- reviewed baseline and implementation commit;
- before/after workflow job graph;
- before/after trigger behavior;
- exact final routine command list;
- classification of every removed job and its preserved local/manual replacement surface;
- proof that the single job has no matrix, artifact, release, audit-install, WASM-install, or cross-build step;
- results of all required verification commands with exit status;
- explicit resource bounds;
- branch-protection disposition;
- confirmation that no test/example source was removed;
- residual risks, especially optional-feature coverage deferred to Milestone 002.

## 16. Handoff notes

This repository has previously experienced severe thread, process, memory, and I/O amplification during broad tests. Do not increase parallelism to recover wall-clock time. The intended reduction comes from removing duplicated runners and optional work, not from allowing more local subprocess fan-out.

Keep the workflow plain. One job with readable steps is preferred over reusable-workflow indirection, generated YAML, composite actions, path matrices, or custom caching logic.

Do not edit `.github/workflows/release.yml` or `.github/workflows/lsp-real-server.yml` in this milestone except to preserve an unrelated user change. Their removal and replacement evidence have separate plans.
