# CI/Test Throughput M003 — Integration-Harness Consolidation Pilot

Status: blocked on M002

Repository planning baseline: `0c896db32d2325da39b129acde7dd406ce472dc4`

Source roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`

Primary class: polish / test architecture.

Hard dependency:

- M002 closed with stable heavy-test resource policy so topology measurements do not mix two independent changes.

## 1. Objective

Prove or reject the hypothesis that Codegg's large number of top-level integration-test crates is a major remaining compile/link bottleneck.

This milestone is deliberately a pilot. It must consolidate one representative compatible family into fewer Cargo test executables while preserving every test body/assertion, then measure whether the reduction is meaningful before authorizing broader repository restructuring.

## 2. Current evidence

The repository records 189 integration test files under `tests/*.rs`. The current routine workspace run links roughly 100 test binaries and still spends approximately nine minutes of the test step outside actual test execution after recent mold/Nextest tuning.

Each top-level Cargo integration target links against the relevant dependency graph independently. Codegg's root package has a large graph including TUI, SQLx, crypto, search, Eggwork client, LSP facade, and other libraries, making repeated test-target linking a plausible structural cost.

## 3. Pilot-family selection

Before editing, use M001 Cargo timing evidence plus a test-file census to select one family that is:

- default-feature routine CI;
- not in the M002 machine-wide heavy/live set;
- comprised of enough separate top-level targets to make executable-count reduction measurable;
- semantically cohesive;
- not split by incompatible `required-features`;
- not dependent on mutually incompatible crate-level attributes.

Good candidates may include projection/replay, Git credential/execution, Tool Program persistence/recovery, or another measured family. The implementer must select based on current timing/count evidence, not this example list.

## 4. Target structure

Preferred shape:

```text
tests/<family>.rs                 # one Cargo integration target
tests/<family>/mod.rs             # optional family module root
tests/<family>/<case>.rs          # existing test bodies moved/included as modules
```

or another conventional Rust module layout with the same property: many source modules, substantially fewer Cargo integration executables.

Preserve useful module/test names in Nextest output. Do not concatenate unrelated files into one monolithic source file.

Common fixture code may be shared only where already natural; deduplication is not the goal of this milestone.

## 5. Work packages

### WP1 — Inventory the selected family

Record:

- current top-level test-target count;
- test count;
- current `cargo nextest ... --no-run` or Cargo no-run build time for the selected family;
- current execution time;
- feature/resource requirements;
- use of `mod common`, crate attributes, env vars, ports, temp dirs, SQLite, and subprocesses.

### WP2 — Build one consolidated harness

Move or module-wire the selected tests under one or a small bounded number of integration targets.

Preserve:

- exact assertions and fixtures;
- test names where practical;
- platform cfgs;
- feature gates;
- process/env isolation assumptions.

If module consolidation creates same-process global-state interference under plain libtest, keep ordinary Cargo execution serial for that target and rely on Nextest's process model only where the actual configuration guarantees separate processes.

### WP3 — Validate selector/tooling compatibility

Update any:

- documented `cargo test --test <name>` commands;
- plans/active architecture references that are operational rather than historical;
- Nextest filters;
- scripts/guards that enumerate target names.

Historical closure evidence must not be rewritten.

### WP4 — Measure before/after

Measure comparable clean/incremental build/link and execution effects.

The pilot is positive only if it produces a meaningful reduction in link/build work without material execution regression or diagnostic degradation.

A suggested decision rule is to require a clear effect larger than normal hosted/local variance, not an arbitrary percentage. Record the absolute seconds and executable-count reduction.

### WP5 — Decide M004

If positive, document the consolidation pattern and classify additional families suitable for M004.

If negative or operationally harmful, revert the topology change, preserve the measurement evidence, close M003 negatively, and block/cancel M004. Do not force broad consolidation because the roadmap predicted it.

## 6. Required verification

Use exact commands for the selected family plus:

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
```

Also run plain Cargo for the consolidated target with serial libtest execution to catch process-global assumptions:

```bash
cargo test --test <consolidated-target> --locked -- --test-threads=1
```

## 7. Acceptance criteria

- No test behavior/assertion is intentionally removed.
- The selected family uses materially fewer Cargo test executables.
- Test names and failure diagnostics remain usable.
- Plain serial Cargo and routine Nextest execution remain green.
- Relevant scripts/docs/selectors are updated without rewriting historical evidence.
- Build/link effect is measured.
- M004 is marked ready only on a positive, reproducible pilot.

## 8. Stop conditions

Stop and close negatively if:

- crate-level cfg/feature differences make consolidation fragile;
- the family relies on incompatible process-global initialization within one executable;
- diagnostics become materially worse;
- the target requires a new test framework;
- compile/link savings are negligible;
- execution time or flake rate regresses enough to erase the build win.

## 9. Closure evidence

Create `plans/closure/ci-test-throughput-optimization/003-status.md` recording:

- selected family and why;
- before/after target and test counts;
- before/after build/link and execution timing;
- selector/documentation changes;
- failure-diagnostic assessment;
- verification results;
- positive/negative pilot disposition;
- M004 status.
