# CI/Test Throughput M004 — Bounded Integration-Test Family Consolidation

Status: blocked/conditional on positive M003

Repository planning baseline: `0c896db32d2325da39b129acde7dd406ce472dc4`

Source roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`

Primary class: polish / test architecture.

Hard dependency:

- M003 closed positively with a measured consolidation pattern.

## 1. Objective

Apply the M003-proven integration-harness pattern to additional compatible root integration-test families, reducing repeated Cargo test executable linking while preserving resource boundaries, feature gates, test semantics, and maintainable failure diagnostics.

This is bounded consolidation, not a rewrite of the test suite.

## 2. Scope selection

Use M001/M003 timing evidence to rank candidate families by:

- number of separate test executables;
- repeated linkage of the root package;
- low execution heaviness;
- common feature set;
- semantic cohesion;
- low process-global/resource conflict risk.

Prefer high-fragmentation fast/default families first.

Explicitly exclude unless separately proven safe:

- real Eggwork/live external-process qualification;
- PTY/interactive process families protected by M002;
- tests with incompatible `required-features`;
- platform-specific targets that benefit from independent compile gating;
- unusually large/failure-prone suites where one executable would materially worsen diagnostics or rerun granularity.

## 3. Work packages

### WP1 — Freeze a consolidation map

Before moving files, create a bounded mapping of current test targets to proposed family harnesses.

The map must state for every moved target:

- destination harness;
- feature/platform cfg;
- resource class;
- plain-Cargo serialization requirement;
- operational command references requiring updates.

Do not set a repository-wide target-count quota. The stopping point is evidence-based.

### WP2 — Consolidate one family at a time

For each family:

1. move/module-wire source without changing assertions;
2. update only live operational selectors;
3. run the family serially with Cargo;
4. run with Nextest `ci`;
5. record build/link effect before proceeding.

If a family regresses safety or offers negligible savings, revert that family without invalidating previously positive families.

### WP3 — Preserve test discoverability

Maintain:

- meaningful module paths;
- test names visible in Nextest output;
- targeted commands for major subsystems;
- no giant generated harness file.

If `cargo test --test old_name` disappears, active docs must point to the replacement selector/module path. Historical plan/closure commands remain untouched.

### WP4 — Re-audit Nextest filters

After target names change, update M002 resource filters/groups and prove they still match the intended tests. A filter that silently matches zero tests is a correctness defect.

### WP5 — Measure the complete workspace

At the final candidate, record:

- root integration-test target count before/after;
- total workspace test executable count where obtainable;
- workspace no-run build/link time;
- Nextest execution time;
- full hosted test-step duration;
- total hosted job duration.

Compare against the M002 baseline, not the historical 37-minute state.

## 4. Required verification

Per-family focused commands plus:

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
```

Run at least one clean or equivalently controlled build measurement to avoid claiming an incremental-cache artifact as a topology win.

Hosted CI must pass on the final candidate.

## 5. Acceptance criteria

- Every moved test remains present and executable.
- Feature/platform/resource boundaries are preserved.
- Final test-target count is lower by a material amount, justified by the selected families.
- Workspace compile/link wall time improves beyond measurement noise.
- Actual test execution does not regress enough to erase the build gain.
- M002 heavy/live filters still select exactly the intended tests.
- Active testing/contributor docs have valid focused commands.
- No new testing framework/dependency is introduced solely for consolidation.

## 6. Stop conditions

Stop further consolidation if:

- the remaining targets are predominantly heavy/specialized;
- the next candidate family would mix incompatible features/resources;
- build/link improvement has flattened;
- executable size/memory causes new runner pressure;
- failure diagnosis or focused iteration becomes materially worse.

It is acceptable for dozens of specialized integration targets to remain. The goal is not one binary; it is removal of wasteful fragmentation.

## 7. Closure evidence

Create `plans/closure/ci-test-throughput-optimization/004-status.md` recording:

- consolidation map;
- families retained/reverted;
- target-count delta;
- no-run/build timing delta;
- execution timing delta;
- hosted CI result;
- selector/filter audit;
- residual specialized targets and why they remain;
- M005 readiness.
