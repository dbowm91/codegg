# Command Routing Roadmap Final Validation and Hygiene Pass

## Objective

Complete the final repository-hygiene work associated with the command-intent, Python-scripting, TestRunner, RunStore, projection, and active-routing roadmap.

The functional roadmap is complete. This pass is not an implementation phase. It exists to remove the remaining validation ambiguity by fixing two pre-existing RunStore tests, updating stale validation metadata, re-running formatting and static checks, and recording an honest capped workspace result.

The target state is:

- the command-routing validation document references the actual implementation commits;
- the full `codegg-core` RunStore test suite completes without hangs or false failures;
- formatting and static validation are re-run against the current commit;
- the capped workspace suite is attempted under the repository’s resource constraints;
- any remaining timeout or environment limitation is documented precisely rather than implied away.

## Scope

This pass is limited to:

1. `fs_store_complete_updates_index` intermittent hang;
2. `mem_store_integrity_violation` incorrect corruption setup;
3. validation-document SHA and result cleanup;
4. formatting, clippy, check, and capped workspace validation;
5. documentation of any genuinely unresolved test-harness limitation.

## Non-goals

- Do not change command-routing behavior.
- Do not expand active routing.
- Do not alter permission policy.
- Do not redesign RunStore persistence.
- Do not add new TUI features.
- Do not add new command families.
- Do not broaden Python sandbox capabilities.
- Do not suppress or ignore failing tests merely to obtain a green suite.

## Current known issues

### 1. `fs_store_complete_updates_index` may hang

The complete `codegg-core` RunStore suite has intermittently stalled in `fs_store_complete_updates_index`.

The likely classes of defect are:

- a lock held across an async or filesystem operation;
- a nested lock acquisition between manifest completion and index update;
- test isolation failure caused by shared filesystem paths;
- cleanup waiting on an open file/handle;
- a retry or atomic-write path that never terminates under a particular error state;
- interaction between serialized tests and a blocking filesystem operation.

The pass must determine the actual cause rather than adding a timeout around the test and calling it fixed.

### 2. `mem_store_integrity_violation` corrupts the wrong field

The test mutates a manifest-level `sha256` field but `read_artifact()` validates against the artifact-store record and recomputed artifact data. The corruption does not affect the actual integrity source, so the assertion expecting an error is invalid.

The test must corrupt the artifact record or artifact bytes used by the integrity check.

### 3. Validation evidence contains stale commit metadata

`docs/validation/command-routing-execution-ownership.md` still contains placeholder or stale SHA text from the implementation pass.

The document must reference:

- `c35a2da2691aa7a83ce61b74396dc6fd848466fc` for canonical delegation;
- `bec25130945b07ed1a2be8dd9c51764e9a660818` for timeout and ownership follow-up;
- the final hygiene-pass commit produced by this work.

### 4. Formatting and full-suite evidence are incomplete

The final delegation validation did not re-verify `cargo fmt --all -- --check`, and the complete RunStore/workspace suite was not cleanly recorded.

## Workstream A: Reproduce and diagnose the filesystem RunStore hang

### Required reproduction

Run the test in isolation repeatedly:

```bash
cargo test -p codegg-core fs_store_complete_updates_index -- --exact --nocapture --test-threads=1
```

If the exact test path differs, use the fully qualified name reported by `cargo test -- --list`.

Run at least:

- one normal isolated invocation;
- a repeated loop of 20–50 invocations;
- the surrounding RunStore module suite;
- the entire `codegg-core` library suite with `--test-threads=1`.

Suggested loop:

```bash
for i in $(seq 1 30); do
  echo "run $i"
  cargo test -p codegg-core fs_store_complete_updates_index -- --exact --test-threads=1 || exit 1
done
```

### Instrumentation

Add temporary or test-only tracing around:

- `begin_run()`;
- artifact writes;
- `complete_run()`;
- manifest rewrite;
- JSONL index rewrite/update;
- lock acquisition and release;
- atomic rename;
- cleanup/drop of temporary directories.

Prefer scoped tracing over arbitrary sleeps.

### Investigation checklist

1. Inspect whether any `parking_lot`/Tokio/RwLock guard is held across `.await`.
2. Check whether `complete_run()` calls a helper that reacquires the same lock.
3. Check whether index update and manifest read/write use inconsistent lock ordering.
4. Verify temporary paths are unique per test.
5. Verify no background cleanup task references the test directory.
6. Verify atomic file replacement does not loop indefinitely on platform-specific rename behavior.
7. Verify test teardown does not block on an open file descriptor.
8. Verify no use of `spawn_blocking` or blocking filesystem calls can starve the configured runtime.

### Fix requirements

The fix must:

- remove the root cause;
- preserve atomic manifest/index behavior;
- preserve concurrent-reader safety;
- avoid adding arbitrary sleeps;
- avoid simply marking the test ignored;
- avoid reducing assertions.

### Regression tests

Add one or more tests for the identified cause, for example:

- completion updates manifest and index under repeated execution;
- concurrent `get_run()`/`list_runs()` during completion does not deadlock;
- index rewrite failure returns a bounded error rather than hanging;
- temporary-directory teardown completes after store drop.

### Acceptance criteria

- isolated test passes repeatedly;
- RunStore module suite completes;
- complete `codegg-core` library suite completes with one test thread;
- no timeout wrapper is required to mask the defect.

## Workstream B: Correct the memory-store integrity test

### Required analysis

Confirm the production integrity contract:

1. locate the authoritative artifact checksum field;
2. verify how `MemRunStore::read_artifact()` obtains stored bytes and expected checksum;
3. verify whether manifest artifact metadata is a copy or the authoritative record;
4. identify the exact mutation required to create a genuine integrity mismatch.

### Required test fix

Change `mem_store_integrity_violation` so it corrupts one of:

- stored artifact bytes while retaining the original checksum; or
- the authoritative artifact checksum while retaining the original bytes.

Do not mutate only an unrelated manifest copy.

### Assertions

The test should assert:

- `read_artifact()` returns the expected integrity error variant;
- the error identifies the artifact/run sufficiently for debugging;
- a valid artifact still reads successfully before corruption;
- range reads also reject corrupted artifacts if integrity is checked before slicing.

### Additional parity check

Add equivalent filesystem-store coverage if not already present:

- corrupt persisted artifact bytes or metadata;
- verify `FsRunStore::read_artifact()` detects the mismatch;
- ensure Mem and Fs stores enforce the same integrity contract.

### Acceptance criteria

- the memory-store test fails before the fix for the correct reason and passes afterward;
- no production integrity check is weakened;
- Mem/Fs behavior is documented and consistent.

## Workstream C: RunStore suite hardening

After fixing both known tests, run the complete RunStore-focused matrix:

```bash
cargo test -p codegg-core run_store -- --test-threads=1
cargo test -p codegg-core --lib -- --test-threads=1
```

Also run relevant focused cases individually:

```bash
cargo test -p codegg-core fs_store_complete_updates_index -- --exact --test-threads=1
cargo test -p codegg-core mem_store_integrity_violation -- --exact --test-threads=1
cargo test -p codegg-core read_artifact -- --test-threads=1
cargo test -p codegg-core concurrent -- --test-threads=1
```

Verify:

- begin/write/complete lifecycle;
- index consistency;
- ranged reads;
- integrity validation;
- path traversal rejection;
- retention/cleanup;
- incomplete-run recovery;
- concurrent reads/writes;
- backward-compatible manifest deserialization.

## Workstream D: Update validation evidence accurately

Update:

```text
docs/validation/command-routing-execution-ownership.md
```

### Required commit metadata

Replace placeholder text with explicit commit history:

- canonical delegation: `c35a2da2691aa7a83ce61b74396dc6fd848466fc`;
- timeout/ownership correction: `bec25130945b07ed1a2be8dd9c51764e9a660818`;
- final validation/hygiene implementation: actual resulting commit SHA.

Since a commit cannot know its own final SHA before creation, use one of these acceptable approaches:

1. make the code/test fixes in one commit, then update the validation document in a second commit with the first commit SHA; or
2. record the validated tree/base SHA and then follow with a metadata-only evidence commit.

Do not leave `pending (this commit)` in the final document.

### Required result categories

Separate results into:

- Passed;
- Failed;
- Timed out;
- Skipped;
- Not applicable.

Do not mark a criterion complete if its required test was not run.

### Required environment fields

Record:

- operating system;
- architecture;
- Rust/Cargo version;
- `CARGO_BUILD_JOBS`;
- test thread count;
- sandbox backend observed for Python tests;
- whether GitHub combined status/checks were available.

### Acceptance criteria

- document references real commits;
- no placeholder SHA remains;
- no contradiction exists between test tables and closure checklist;
- pre-existing warnings/issues are either fixed or explicitly scoped.

## Workstream E: Formatting and static validation

Run against the final code state:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If all-feature clippy is blocked by known unrelated warnings:

1. capture the exact warnings;
2. determine whether they are truly pre-existing on the base commit;
3. fix low-risk warnings where practical;
4. do not use broad `allow` attributes merely to obtain green output;
5. record any intentionally deferred warning with file, line, lint, and rationale.

The preferred closure state is zero warnings.

## Workstream F: Capped full workspace validation

Respect the repository’s known resource constraints:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

Where useful, split heavy feature paths into separate invocations rather than allowing concurrent build/test pressure:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features --no-run
```

If the full suite exceeds the available execution window:

- record the exact point reached;
- record elapsed wall-clock time;
- identify the last running test/binary;
- distinguish timeout from hang;
- ensure all command-routing, Python, TestRunner, RunStore, and projection suites complete separately.

Do not classify a timed-out full suite as passed.

## Workstream G: Focused roadmap regression matrix

Re-run the roadmap-critical suites after RunStore changes:

```bash
cargo test --test command_routing_execution_ownership -- --test-threads=1
cargo test --test command_routing_adversarial -- --test-threads=1
cargo test --test python_sandbox_adversarial -- --test-threads=1
cargo test --test context_projection_adversarial -- --test-threads=1
cargo test -p codegg --lib tool::bash -- --test-threads=1
cargo test -p codegg --lib python_script -- --test-threads=1
cargo test -p codegg --lib test_runner -- --test-threads=1
```

Acceptance criteria:

- no regression in canonical delegation;
- no duplicate run records;
- raw-shell run kinds remain unconditional;
- Python and TestRunner ownership remain truthful;
- raw artifacts remain unsafe for model context;
- permission defaults remain unchanged.

## Workstream H: Documentation reconciliation

Update only documentation affected by the test fixes and final validation:

- `docs/validation/command-routing-execution-ownership.md`;
- `architecture/run_store.md` if the deadlock/integrity fix changes an invariant;
- `AGENTS.md` only if contributor guidance needs a new RunStore testing note.

Document:

- lock ordering or async-lock rule identified by the filesystem fix;
- authoritative checksum source;
- recommended serialized RunStore test command;
- known resource constraints for full workspace testing.

Avoid adding broad roadmap prose now that the roadmap is complete.

## Recommended implementation order

1. Reproduce `fs_store_complete_updates_index` repeatedly.
2. Instrument and fix the underlying hang.
3. Correct `mem_store_integrity_violation` to corrupt the authoritative data/checksum.
4. Add regression/parity tests.
5. Run the complete `codegg-core` RunStore suite.
6. Re-run roadmap-critical suites.
7. Run fmt, check, and clippy.
8. Attempt the capped full workspace suite.
9. Update validation evidence with actual commits and exact results.
10. Commit any metadata-only validation correction separately if needed.

## Closure criteria

This hygiene pass is complete when:

- `fs_store_complete_updates_index` passes repeatedly without hanging;
- `mem_store_integrity_violation` tests the real integrity boundary and passes;
- complete `codegg-core` RunStore tests finish with `--test-threads=1`;
- command-routing ownership and adversarial suites still pass;
- `cargo fmt --all -- --check` passes;
- workspace check passes;
- clippy passes, or any unrelated exception is precisely documented;
- the capped workspace suite is run and truthfully classified;
- validation evidence contains no placeholder SHA and matches the tested code;
- no command-routing roadmap functionality remains open.
