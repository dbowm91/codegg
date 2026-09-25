# CI/Test Throughput M005 — Cache and Critical-Path Closure

Status: blocked on M004 or documented negative M003/M004 disposition

Repository planning baseline: `0c896db32d2325da39b129acde7dd406ce472dc4`

Source roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`

Primary class: polish / development infrastructure.

## 1. Objective

With compiler/profile settings, heavyweight-test policy, and test-target topology stabilized, evaluate the remaining non-structural CI opportunities and close the workstream with an explicit measured routine-CI contract.

This milestone may evaluate compiler-result caching and safe critical-path overlap. It must not compensate for unresolved structural waste by adding a complex build system.

## 2. Preconditions

M005 begins only after:

- M001 has fixed the CI test profile/build-job settings;
- M002 has fixed heavy/live scheduling and fixture setup;
- M003 has produced a positive or negative consolidation result;
- M004 is closed if M003 was positive, or explicitly marked unnecessary/blocked if M003 was negative.

Use the resulting hosted run as the new baseline.

## 3. Work packages

### WP1 — Measure remaining critical path

Classify current wall time into:

- checkout/toolchain/cache/setup;
- static guards/format;
- Clippy;
- test compile/link;
- test execution;
- explicit live qualification if triggered;
- cache save/restore overhead.

Do not optimize steps that are already sub-second unless the change simplifies the workflow.

### WP2 — Evaluate compiler-result caching

A bounded `sccache` experiment is permitted.

Requirements:

- use a supported GitHub Actions cache/storage backend without external credentials where possible;
- record cache hit/miss statistics;
- compare at least cold/initial and subsequent changed-source runs;
- ensure cache restore/save overhead does not erase the benefit;
- preserve Cargo lock/profile/toolchain identity in cache correctness;
- do not retain sccache if the hit rate or wall-time benefit is poor.

Do not add a separately operated cache service.

### WP3 — Evaluate same-job critical-path overlap

Only independent non-mutating steps may overlap.

Candidates may include cheap static guards or formatting versus compilation, but Cargo operations that contend on the same target directory must not be naively backgrounded against one another.

If safe overlap requires opaque shell process management, complicated log multiplexing, or failure-cancellation machinery, reject it. Workflow simplicity is an explicit constraint.

A second permanent GitHub job solely to parallelize Clippy/tests is outside scope because it duplicates the dominant compile graph unless a future separately approved plan demonstrates artifact sharing that eliminates that duplication.

### WP4 — Clean up superseded experiments

Remove:

- temporary timing instrumentation not useful for local diagnostics;
- comments describing current settings as probes/experiments;
- stale Nextest filters or cache options;
- any candidate cache configuration that was tested and rejected.

Do not leave multiple undocumented performance modes.

### WP5 — Reconcile the verification contract

Update:

- `architecture/testing.md`;
- `.github/workflows/ci.yml` comments;
- `AGENTS.md` / `CONTRIBUTING.md` only where commands/resource policy changed;
- `scripts/verify.sh` only if local behavior intentionally changed.

Document:

- ordinary PR scope;
- change-sensitive live qualification;
- main/Linux authority where applicable;
- cache behavior;
- final measured wall time and resource policy.

## 4. Required verification

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
git diff --check
```

Plus explicit supported-Linux live qualification according to M002's final policy.

Hosted CI evidence is mandatory for closure.

## 5. Acceptance criteria

- Final routine CI remains one bounded non-release job.
- Any compiler-result cache retained has measured positive wall-time effect and visible statistics.
- No cache requires external secrets/service operation.
- Any overlap retained is simple, deterministic, and failure-transparent.
- No test, lint, guard, or authoritative qualification was silently lost.
- Active documentation matches the exact final workflow.
- Final closure compares the new timing against both the M001 baseline and the original ~19-minute reviewed state.
- Temporary experiments are removed.

The workstream should aim for a routine steady-state PR feedback loop below 15 minutes, but closure is based on correctness-preserving measured improvement rather than a hard performance gate.

## 6. Stop conditions

Stop and reject a candidate if:

- it adds more workflow/build complexity than the measured benefit justifies;
- caching causes stale/incorrect build reuse or requires broad key invalidation hacks;
- overlapping steps contend on Cargo target locks or reduce failure clarity;
- a second CI job is needed without eliminating duplicated compilation;
- the only remaining path to a lower number is dropping correctness coverage.

## 7. Closure evidence

Create `plans/closure/ci-test-throughput-optimization/005-status.md` including:

- final workflow topology;
- final profile/build/Nextest settings;
- cache statistics and disposition;
- any overlap experiment and disposition;
- before/after wall times with cache-state notes;
- final test count and live qualification policy;
- verification/hosted CI results;
- unresolved findings by severity;
- final recommendation: closed, conditionally closed, or additional separately scoped work required.
