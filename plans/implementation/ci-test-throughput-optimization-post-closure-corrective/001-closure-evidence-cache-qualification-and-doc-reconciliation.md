# CI/Test Throughput Post-Closure Corrective C001 — Closure Evidence, Cache Qualification, and Documentation Reconciliation

Status: blocked on C002 hosted CI timing-flake stabilization

Repository baseline reviewed: `4f7e508976e70fbec0e645bd530b63fe3c8393c1`

Source corrective addendum:

- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`

Predecessor closure:

- `plans/closure/ci-test-throughput-optimization/005-status.md`

Primary class: polish / development infrastructure.

Hard dependency:

- C002 hosted CI timing-flake stabilization must close first:
  `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/002-hosted-ci-timing-flake-stabilization.md`.

C001 MUST NOT begin the sccache A/B or use hosted timing runs for closure while C002 is open. Planning/documentation inventory work may be reviewed, but final timing/cache evidence resumes only after C002 restores a trustworthy green main/live baseline.

## 1. Objective

Produce an evidence-correct final closure for the CI/test-throughput campaign without reopening its successful implementation scope.

C001 owns four defects found after M005 closure:

1. predecessor roadmap/registry status drift and duplicate stale M004/M005 milestone blocks;
2. stale values and incomplete step ordering in the primary `architecture/testing.md` CI section;
3. M005's unmeasured `sccache` rejection based on cache-equivalence/credential assumptions not supported by current upstream documentation;
4. absence of a direct hosted measurement for the unrelated-PR path that omits live Eggwork qualification.

## 2. Current implementation evidence

At baseline `4f7e5089`:

- `.github/workflows/ci.yml` has one `verify` job;
- hosted compile jobs are fixed at `CARGO_BUILD_JOBS=4`;
- Nextest `ci` uses 4 slots;
- five heavy binaries retain `threads-required = "num-cpus"`;
- M002 prebuilds and caches the workspace-excluded Eggwork fixture;
- unrelated PRs may exclude `eggwork_remote_execution_live`;
- relevant PRs and all main pushes include it;
- M003 reduced eleven `projection_replay_*` Cargo targets to one;
- M004 reduced five default-feature `session_*` targets to one;
- final predecessor main/live run `36196068239` attempt 3 is green at 17m17s, with a 7m51s test build and 362.537s execution for 11,726 passed / 1 skipped.

The remaining build phase is still the largest single cost.

## 3. Correctness and compatibility invariants

Do not change:

- product code or runtime behavior;
- test assertions or intended test count except unavoidable harness-discovery corrections explicitly proven equivalent;
- M002 live relevance selectors except to fix a correctness bug discovered by this pass;
- M002 heavy-test exclusivity;
- M003/M004 target topology;
- release permissions or workflow authority;
- local quick/full verification semantics;
- supported-Linux/main live qualification.

A cache candidate must be removable in one revert/config edit and must not become necessary for correctness.

## 4. Work packages

### WP1 — Reconcile planning state without rewriting history

Correct the predecessor control surfaces:

- mark `plans/subsystems/ci-test-throughput-optimization-roadmap.md` as historically closed, with a pointer to this post-closure corrective;
- remove the stale duplicate M004/M005 milestone blocks that still report blocked states;
- ensure the roadmap dependency/milestone summary names M001-M005 as closed;
- update `plans/registry.md` so the original workstream is closed and this corrective C001 is the only active/ready CI-throughput handoff;
- preserve `plans/closure/ci-test-throughput-optimization/001-status.md` through `005-status.md` byte-for-byte unless a broken link alone requires an additive pointer elsewhere.

Regression guard:

```bash
rg -n 'Status: active|Status: blocked/conditional on positive M003|Status: blocked on M004' \
  plans/subsystems/ci-test-throughput-optimization-roadmap.md
```

Expected after reconciliation: no stale predecessor status matches.

### WP2 — Reconcile active CI/testing documentation

Update the authoritative operational section of `architecture/testing.md` so it agrees with the actual workflow before any cache experiment:

- final retained build setting: `CARGO_BUILD_JOBS=4`;
- final predecessor main/live baseline: 17m17s;
- final predecessor test count: 11,726 passed / 1 skipped;
- current test-target count after M003/M004 consolidation where documented;
- explicit steps for live-Eggwork relevance detection and conditional fixture prebuild;
- distinguish main/relevant-PR live path from unrelated-PR fast path;
- retain historical ~37-minute and ~19-minute figures only when clearly labeled as historical baselines, not current state.

Audit all active references:

```bash
rg -n --glob '!plans/archive/**' --glob '!plans/closure/**' \
  'JOBS=8|~19 min|11781|11,781|~100 test binaries|M005.*ready|M004.*blocked' \
  architecture AGENTS.md CONTRIBUTING.md .github .config scripts plans/subsystems plans/registry.md
```

Classify every remaining match as historical, current, or stale.

### WP3 — Preregister the compiler-cache A/B before running it

Before modifying hosted CI, record the candidate and decision rule in the C001 working/closure evidence.

Control A:

- current workflow;
- `Swatinem/rust-cache@v2`;
- no `RUSTC_WRAPPER`;
- current `CARGO_BUILD_JOBS=4`;
- current Nextest/resource policy.

Candidate B:

- preserve `Swatinem/rust-cache` for dependency/target reuse;
- add `sccache` as the Rust compiler wrapper;
- use the supported GitHub Actions cache backend and Actions-provided runtime credentials; no user-managed/external secret;
- keep the same Rust toolchain, Cargo profile, build jobs, test scope, and Nextest policy;
- ensure `CARGO_INCREMENTAL=0`/cache interaction remains explicit and compatible.

Use the current supported upstream setup rather than hardcoding a stale action version in this plan. Pin the selected action/version according to repository convention if retained.

Required statistics:

```bash
sccache --show-stats
```

Capture at minimum:

- compile requests;
- cache hits/misses;
- non-cacheable calls;
- cache read/write errors;
- cache size/overhead where available;
- Workspace Clippy wall time;
- workspace-test build phase;
- Nextest execution time;
- total job time.

### WP4 — Execute comparable hosted cache measurements

A single cold candidate run is not acceptance evidence.

Minimum sequence:

1. control/reference run on the stabilized non-sccache configuration, or use a sufficiently recent comparable warm control if its cache/toolchain/workflow identity is unchanged;
2. candidate seed/cold run;
3. candidate warm run;
4. a second candidate run after a small ordinary source change that leaves most workspace crates unchanged, so compiler-output reuse can actually be observed;
5. if runner variance makes the result ambiguous, one additional control/candidate comparison.

Do not compare a cold control to a warm candidate as proof.

Preregistered retention threshold:

- candidate must reduce the warm workspace-test build phase by at least **45 seconds** relative to comparable control evidence;
- candidate must reduce total warm job wall time by at least **30 seconds**;
- the improvement must appear in at least two comparable candidate observations rather than one outlier;
- cache/setup/save overhead must not erase the gain;
- no new flake, stale-build behavior, cache error, or test-scope change.

If these gates are not met, fully revert the sccache candidate and close with a measured negative disposition.

The threshold is intentionally modest relative to the 7m51s build phase but large enough to exceed ordinary tens-of-seconds runner noise.

### WP5 — Measure the unrelated-PR fast path

Capture at least one green pull-request run where the repository-owned live-change detector reports that `eggwork_remote_execution_live` is omitted.

Use a controlled, non-live-relevant source change. An ephemeral measurement branch/PR is acceptable; the final repository tree must not retain a meaningless source change solely to preserve the timing run.

Record:

- detector result;
- fixture-prebuild skipped/included state;
- Clippy time;
- test build phase;
- Nextest execution;
- total job wall time;
- test count;
- cache state;
- whether sccache was control or retained candidate.

Do not use a docs-only PR because `paths-ignore` intentionally skips CI.

This measurement becomes the ordinary unrelated-PR baseline. Main/live remains the authoritative integration baseline.

### WP6 — Finalize the cache and documentation disposition

If sccache passes WP4:

- retain the minimal workflow configuration;
- document why `rust-cache` and `sccache` have distinct roles;
- document GHA cache statistics and invalidation behavior;
- keep configuration simple and visible.

If sccache fails WP4:

- remove all candidate workflow/env/action configuration;
- retain only the closure evidence showing the measured negative result;
- correct the old M005 reasoning so active docs no longer claim cache equivalence or required external secrets.

In either case:

- `architecture/testing.md` must state both final baselines:
  - ordinary unrelated-PR path;
  - main/relevant-change live path;
- predecessor M005 closure remains unchanged and is explicitly superseded only for its compiler-cache disposition.

### WP7 — Closure and final registry reconciliation

Create:

- `plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-status.md`.

Then:

- mark the corrective addendum C001 closed;
- mark the registry corrective row closed or move it to recently closed per current registry convention;
- mark the original CI/test throughput workstream closed, not closing;
- leave no ready/blocked milestone in this line unless the C001 evidence discovers a new correctness defect requiring a separately scoped plan.

## 5. Required verification

Repository consistency:

```bash
git diff --check
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
```

Planning/documentation checks:

```bash
rg -n 'CI and test throughput optimization|CI/test throughput' \
  plans/registry.md plans/subsystems/ci-test-throughput-optimization*.md

rg -n --glob '!plans/archive/**' --glob '!plans/closure/**' \
  'JOBS=8|~19 min|11781|11,781|M005.*ready|M004.*blocked' \
  architecture AGENTS.md CONTRIBUTING.md .github .config scripts plans
```

Hosted operational evidence:

- one green main/relevant-change run with live Eggwork qualification;
- one green unrelated-PR run with live target omitted;
- the WP4 cache A/B sequence with visible `sccache --show-stats` if the candidate is exercised.

If the final tree rejects sccache, the final main/live run must be on the reverted non-sccache configuration.

## 6. Failure, cancellation, restart, and contention semantics

- Superseded hosted runs remain cancelable through the existing workflow concurrency group.
- A cancelled/superseded measurement is not evidence.
- Cache failure must fail open only with respect to performance, never correctness: Cargo must still compile normally or the candidate is rejected.
- Do not share a cache namespace across incompatible Rust toolchains/profiles without the backend's normal content/key isolation.
- Do not parallelize Clippy and tests against the same target directory as part of this corrective.
- Temporary measurement PRs/branches must not become runtime dependencies.

## 7. Security and supply-chain constraints

- No external cache service credentials.
- No repository secrets added for sccache.
- Any new GitHub Action used for the experiment/retained configuration must be a recognized upstream action and pinned according to repository convention.
- Workflow permissions remain `contents: read`.
- Do not upload arbitrary build artifacts to a public external service.
- Cache reuse must remain scoped to GitHub Actions-supported storage and normal Rust/Cargo identity/invalidation semantics.

## 8. Documentation effects

Expected active-document changes:

- `architecture/testing.md`;
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`;
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`;
- `plans/registry.md`;
- `.github/workflows/ci.yml` only if the measured cache candidate is retained or temporary experiment commits require it during evidence collection.

Review but do not change unless stale:

- `AGENTS.md`;
- `CONTRIBUTING.md`;
- `scripts/verify.sh`;
- `.config/nextest.toml`.

Historical closure records M001-M005 remain immutable.

## 9. Acceptance criteria

- Original M001-M005 roadmap is clearly closed and has no duplicate stale active milestone blocks.
- Registry has one unambiguous status for the original workstream and C001.
- Primary CI documentation uses current JOBS/test counts/step ordering and clearly labels historical numbers.
- An unrelated-PR fast-path hosted baseline is recorded.
- `sccache` is actually evaluated on hosted GitHub Actions with visible stats.
- Final cache disposition is based on the preregistered threshold and comparable warm evidence.
- If rejected, no sccache configuration remains in the final workflow.
- If retained, no external secret/service is required and workflow permissions remain read-only.
- Main/live hosted CI is green on the final configuration.
- No product/test assertion/resource-topology regression.
- C001 closure states precisely which M005 cache conclusions are superseded.

## 10. Stop conditions

Stop and report rather than widen scope if:

- sccache requires an external service/credential under the selected supported backend;
- cache integration requires changing Rust profiles, test topology, or build jobs to manufacture a win;
- a cache hit produces stale/incorrect output;
- the unrelated-PR selector omits live qualification for a change that should require it;
- planning cleanup reveals another active plan that depends on the stale M004/M005 status;
- hosted failures indicate a product/test correctness defect unrelated to cache/documentation work;
- the only path to the timing threshold is reducing coverage.

## 11. Closure evidence required

The closure record must contain:

- exact implementation/measurement commits;
- exact hosted run IDs and attempts;
- control vs candidate cache states;
- `sccache --show-stats` from each meaningful candidate run;
- control/candidate Clippy, test-build, test-execution, and total times;
- retention-threshold calculation;
- final cache disposition;
- unrelated-PR detector output and wall time;
- main/live final wall time;
- planning/documentation consistency search results;
- proof historical M001-M005 closure records were not rewritten;
- unresolved findings by severity;
- final workstream disposition.
