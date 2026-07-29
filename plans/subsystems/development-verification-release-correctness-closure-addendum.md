# Development Verification and Release Correctness Closure Addendum

Status: active

Parent roadmap:

- `plans/subsystems/development-verification-release-roadmap.md`

Historical implementation plans:

- `plans/implementation/development-verification-release/001-routine-ci-contraction.md`
- `plans/implementation/development-verification-release/002-local-verification-contract.md`
- `plans/implementation/development-verification-release/003-manual-crates-io-release-ownership.md`
- `plans/implementation/development-verification-release/004-integration-evidence-cleanup-and-closure.md`

Historical closure records:

- `plans/closure/development-verification-release/001-status.md`
- `plans/closure/development-verification-release/002-status.md`
- `plans/closure/development-verification-release/003-status.md`
- `plans/closure/development-verification-release/004-status.md`

Corrective implementation plan:

- `plans/implementation/development-verification-release/005-green-verification-and-crates-io-closure.md`

Target independent closure record:

- `plans/closure/development-verification-release/005-status.md`

Long-term references:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#3-non-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related ADRs:

- None. This addendum restores the already-approved ownership model: bounded hosted verification and maintainer-operated crates.io publication. It does not authorize hosted publication or introduce a new build system.

## 1. Purpose

The original roadmap achieved the intended structural reduction, but post-implementation review found that strict closure was not supported by the repository evidence.

This addendum reopens only the correctness and ownership boundary needed to make the reduced apparatus operational. It does not restore the deleted CI matrix, release workflow, LSP evidence workflow, artifact aggregation, repeated plugin lanes, audit lane, example matrix, or cross-target hot-path builds.

The governing rule remains:

> Keep the reduced maintenance surface, but require every canonical verification gate and the approved manual crates.io release path to work as documented before strict closure.

## 2. Post-closure findings

### F01 — The sole routine CI job is deterministically failing at the Tokio flavor guard

`.github/workflows/ci.yml` invokes:

```bash
python3 scripts/check-tokio-test-flavors.py
```

The guard returns nonzero when any bare `#[tokio::test]` annotation exists. The M001 and M002 closure records report 1,062 pre-existing bare annotations and record that the command exits 1. No checked-in baseline or allowlist makes the guard operate as a new-regression check.

Impact:

- the single hosted verification job cannot reach later steps;
- `scripts/verify.sh quick` cannot pass;
- the repository describes the script as canonical while its normal success path is unavailable.

### F02 — The default workspace test command has a known failing test

The M001 and M002 closure records report that:

```text
tool::bash::tests::active_mode_python_command_routes
```

panics because Python execution requires scheduler admission while the scheduler is disabled in the test harness.

Impact:

- the routine CI test step exits nonzero after F01 is fixed;
- `scripts/verify.sh full` cannot pass;
- the failure touches the scheduler-ownership boundary and must not be hidden by excluding the test or bypassing admission.

### F03 — Local full verification omits the stack bound required by the same workspace tests

The routine workflow sets:

```text
RUST_MIN_STACK=33554432
```

because daemon-socket integration tests otherwise abort with stack overflow. `scripts/verify.sh full` runs the workspace tests without exporting that bound.

Impact:

- hosted and local verification do not implement one resource contract;
- local full verification may abort even after F01 and F02 are corrected.

### F04 — Milestone 003 reversed the approved crates.io ownership policy

The parent roadmap and M003 plan require a maintainer-operated crates.io publication procedure. The implementation instead marked every workspace crate `publish = false` and made `RELEASING.md` state that crates.io publication is unsupported.

The M003 closure treated intentional `cargo publish --dry-run` rejection as passing evidence.

Impact:

- the requested crates.io distribution capability is absent;
- the implementation contradicts the roadmap objective and maintainer directive;
- package dry-run rejection cannot satisfy a requirement for successful package/publication dry-runs.

### F05 — Planning state overclaims strict closure

The registry and parent roadmap report the subsystem closed even though the closure records contain unresolved medium findings and contradictory command results.

Impact:

- the active planning control surface is not evidence-faithful;
- downstream agents may treat a failing verification and release contract as settled architecture.

## 3. Corrective ownership boundary

Milestone 005 owns:

- making the retained one-job CI topology green-capable without broadening it;
- turning the Tokio flavor check into a real baseline-aware regression guard;
- repairing the scheduler-dependent Bash/Python routing test without bypassing scheduler authority;
- unifying local and hosted resource bounds;
- restoring a successful manual crates.io packaging and dry-run path for the intended CodeGG distribution package and its required dependency graph;
- reconciling active documentation and planning state;
- producing one independent strict closure record based on successful commands.

Milestone 005 does not own:

- bulk conversion of all historical Tokio tests merely for cleanup;
- reintroducing deleted workflow jobs or artifact evidence systems;
- automated crates.io publishing, trusted publishing, OIDC, release-plz, cargo-release, or tag-triggered releases;
- choosing a release version or performing an actual publication;
- unrelated test-suite cleanup;
- LSP real-server automation;
- supply-chain signing, SBOMs, installers, package-manager integrations, or platform notarization;
- bypassing scheduler admission to make a test pass.

## 4. Corrective target state

### Routine hosted verification

- exactly one ordinary `verify` job remains;
- pull requests and pushes to `main` invoke it;
- it uses read-only permissions;
- it has no matrix, release build, artifact upload, audit installation, real-server installation, example matrix, or duplicated subsystem reruns;
- every command in the job exits zero on the accepted implementation revision;
- deliberately introduced new violations still fail the relevant guard or test.

### Local verification

- `scripts/verify.sh quick` exits zero from the repository root and a nested directory;
- `scripts/verify.sh full` exits zero from a clean checkout;
- broad commands use `CARGO_BUILD_JOBS=1`, `--test-threads=1`, and the accepted `RUST_MIN_STACK` bound;
- local and hosted commands remain semantically aligned;
- no command is marked successful when it was not run or returned nonzero.

### Tokio flavor regression policy

- historical bare Tokio tests are represented by an explicit, reviewable baseline keyed by stable test identity rather than fragile line number;
- the current baseline passes;
- a newly added bare Tokio test fails;
- converting or removing a baseline violation can be detected and the baseline can be intentionally reduced;
- no blanket ignore of the repository or unconditional success path is permitted.

### Scheduler-owned Bash/Python routing

- the failing test no longer panics;
- execution tests use a production-shaped scheduler/admission fixture;
- routing-only assertions are separated from execution when that is the actual contract being tested;
- scheduler unavailability produces a typed refusal/error rather than a panic;
- no direct process-spawn or Python-execution bypass is introduced.

### Manual crates.io release ownership

- GitHub Actions remains unable to publish or create releases;
- at least the intended installable CodeGG package is crates.io-publishable;
- every package required by its registry package graph is either publishable with complete metadata and path-plus-version dependencies or removed from that registry dependency graph through an explicitly reviewed design;
- genuinely private/dev-only workspace packages remain `publish = false`;
- all intended publishable packages pass `cargo package` and `cargo publish --dry-run` in dependency order;
- `RELEASING.md` documents the actual package order, immutable-version handling, index propagation, yanking, tags, and optional manual GitHub releases;
- no actual publication occurs during implementation.

If a required crate name is unavailable or the maintainer lacks crates.io ownership, implementation must stop and report the exact package and ownership conflict. It must not silently revert to a no-crates.io policy or rename packages without approval.

## 5. Dependency graph

```text
Historical M001–M004 implementation
        |
        v
Post-closure correctness review (F01–F05)
        |
        v
M005 — Green verification and crates.io correctness closure
        |
        v
Independent M005 closure review
```

M005 is dependency-ready against repository baseline `942593852057dbd0914066a609e02ca57a016abf`.

## 6. Verification strategy

Strict closure requires all of the following:

1. Tokio guard self-tests and repository-baseline tests demonstrate both current success and new-violation failure.
2. The scheduler-dependent targeted test passes and a negative scheduler-unavailable test returns a typed error without panic.
3. `scripts/verify.sh quick` exits 0.
4. `scripts/verify.sh full` exits 0 with the canonical resource environment.
5. The production-feature compile check passes.
6. Every intended publishable package passes package inspection, `cargo package`, and `cargo publish --dry-run` in dependency order.
7. Workflow searches prove no hosted release authority, write permission, artifact release path, or registry credential exists.
8. The implementation commit has one successful GitHub Actions `verify` result. Absence of an observable hosted run blocks strict closure unless a separate reviewer can produce equivalent GitHub Actions evidence.
9. Active documentation and planning state agree with the commands and package policy actually implemented.

A closure record may not convert a nonzero command into `pass` by labeling it pre-existing. A known external registry or hosted-run limitation must be recorded as a blocker, not as strict closure.

## 7. Completion definition

This addendum is closed only when:

- F01–F05 are resolved;
- the one-job CI architecture remains intact;
- quick and full local verification both pass;
- the hosted `verify` job passes;
- the scheduler authority invariant is preserved;
- manual crates.io packaging and dry-runs succeed for the intended package graph;
- no automated publication authority is introduced;
- active documentation is evidence-faithful;
- a separate reviewer writes `plans/closure/development-verification-release/005-status.md` and finds no unresolved high or medium issue.

## 8. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 001 — Routine CI contraction | conditionally closed | `plans/implementation/development-verification-release/001-routine-ci-contraction.md` | `plans/closure/development-verification-release/001-status.md` | F01–F03 transferred to M005 |
| 002 — Canonical local verification contract | conditionally closed | `plans/implementation/development-verification-release/002-local-verification-contract.md` | `plans/closure/development-verification-release/002-status.md` | F01–F03 transferred to M005 |
| 003 — Manual crates.io release ownership | conditionally closed | `plans/implementation/development-verification-release/003-manual-crates-io-release-ownership.md` | `plans/closure/development-verification-release/003-status.md` | F04 transferred to M005 |
| 004 — Optional integration evidence cleanup and closure | conditionally closed | `plans/implementation/development-verification-release/004-integration-evidence-cleanup-and-closure.md` | `plans/closure/development-verification-release/004-status.md` | F05 transferred to M005; structural cleanup retained |
| 005 — Green verification and crates.io correctness closure | ready for handoff | `plans/implementation/development-verification-release/005-green-verification-and-crates-io-closure.md` | — | — |
