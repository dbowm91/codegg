# Runtime Safety, Resource Control, and Footprint Milestone 002 — Canonical Bounded Process Execution

Status: blocked on M001

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 002

Hard dependency:

- M001 — Landlock and sandbox contract correction must close so this milestone can carry the accepted sandbox request/outcome contract through the canonical executor.

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Primary class: execution invariant and shared infrastructure

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/002-status.md`

Second-person or second-agent correctness review: required for output limits, timeout, cancellation, and descendant cleanup

## 1. Objective

Make `ManagedProcessService`, or a narrowly extracted core of it, the single supervised subprocess boundary for CodeGG's finite command execution paths.

The canonical boundary must provide:

- typed executable and argv;
- explicit cwd and environment policy;
- stdin handling;
- sandbox requirement and obtained outcome from M001;
- streaming stdout and stderr collection with hard byte bounds applied during collection;
- timeout and cancellation ownership;
- process-group or process-tree cleanup;
- exit, spawn, timeout, cancellation, output-limit, and sandbox failure distinction;
- job/session/tool provenance needed by existing scheduler and audit paths.

This milestone must migrate the ad hoc shell, native-command, and Python subprocess paths that currently use `Command::output()` or equivalent unbounded collection. It must also inventory other spawn sites and either migrate them or record a narrow reason they are a different lifecycle class.

## 2. Explicit non-goals

This milestone must not:

- redesign the scheduler, job store, Tool Programs interpreter, provider runtime, ACP, or frontend projections;
- remove raw shell execution;
- change command approval or risk classification;
- redesign argv routing assigned to M003 beyond the minimum typed request needed by the executor;
- convert persistent daemons or language servers into finite commands when their lifecycle differs;
- create a universal operating-system process abstraction beyond CodeGG's actual supported needs;
- add an external process supervisor, container runtime, cgroup manager, or service manager dependency;
- capture unlimited output to disk as a substitute for bounded memory;
- add long-running stress tests or a CI platform matrix;
- change manual release cadence.

## 3. Current implementation evidence

Inspect every production occurrence of:

- `std::process::Command`;
- `tokio::process::Command`;
- `.output()`;
- `.wait_with_output()`;
- `.spawn()` followed by custom stdout/stderr collection;
- `kill_on_drop`;
- `pre_exec`, `setsid`, `setpgid`, or process-group signals;
- direct child kill/wait logic;
- temp-script launch;
- scheduler executor process launch.

Minimum known areas:

- `src/managed_process.rs`;
- `src/tool/bash.rs`;
- `src/python_script/executor.rs`;
- `src/scheduler/executors.rs`;
- LSP/server/helper launch paths found by inventory;
- `docs/execution-ownership.toml`;
- `scripts/check_execution_ownership.py`;
- `tests/managed_process_descendants.rs` and adjacent execution tests.

The reviewed baseline shows:

1. `src/managed_process.rs` already contains the strongest execution lifecycle and Unix descendant-cleanup foundation.
2. `src/tool/bash.rs` raw-shell and native paths use unbounded output collection and truncate only after the child has exited.
3. `src/python_script/executor.rs` also uses unbounded output collection.
4. `kill_on_drop` protects only the directly managed child and is not a complete descendant-cleanup contract.
5. several call paths duplicate spawn, timeout, output formatting, or cleanup semantics.
6. execution ownership guards exist, but they do not yet ensure all governed finite-process paths use one canonical service or reject unbounded collection.

Confirm the inventory against current main before implementation and classify each spawn site as:

- finite governed command — must migrate;
- long-lived managed service — must use an existing explicit lifecycle owner or be documented;
- build/test fixture — may remain local to tests;
- private M001 sandbox helper — participates beneath the canonical parent service;
- platform bootstrap/self-reexec — document and review separately.

## 4. Invariants that cannot regress

- daemon/scheduler ownership remains authoritative for durable jobs;
- cwd comes from typed project/workspace context, never process-global mutation;
- cancellation is owner-scoped and does not terminate unrelated jobs;
- output limits apply separately to stdout and stderr before buffers exceed their configured bounds;
- target exit code remains distinguishable from helper/setup failure;
- timeout remains distinguishable from user cancellation;
- child descendants do not survive a completed timeout/cancel cleanup on supported Unix hosts;
- cleanup always reaps the direct child;
- process-group signaling cannot target the CodeGG daemon's own group;
- raw shell, native command, Python, and scheduler execution preserve their existing approval/authority semantics;
- sandbox outcome from M001 is retained in the final execution result;
- no governed path bypasses the canonical service solely for convenience.

## 5. Required execution contract

Introduce or normalize a request/result contract equivalent to:

```rust
struct ManagedProcessRequest {
    executable: OsString,
    argv: Vec<OsString>,
    cwd: PathBuf,
    env: EnvironmentPolicy,
    stdin: StdinPolicy,
    timeout: Option<Duration>,
    cancellation: CancellationOwner,
    output: OutputPolicy,
    process_tree: ProcessTreePolicy,
    sandbox: SandboxRequest,
    provenance: ExecutionProvenance,
}

struct OutputPolicy {
    stdout_limit: usize,
    stderr_limit: usize,
    overflow: OverflowPolicy,
}

enum ManagedProcessTermination {
    Exited(ExitStatus),
    TimedOut,
    Cancelled,
    OutputLimitExceeded { stream: OutputStream },
    SpawnFailed,
    SandboxFailed,
    CleanupFailed,
}
```

Exact names may follow current types. Avoid duplicate near-equivalent request/result types in shell, Python, and scheduler modules.

The result must carry:

- bounded stdout/stderr bytes or decoded lossy text according to existing API needs;
- per-stream truncation flags and retained-byte counts;
- termination reason;
- duration;
- sandbox outcome;
- cleanup diagnostics when descendants could not be confirmed terminated;
- existing provenance/audit identifiers.

## 6. Output collection design

### 6.1 Stream incrementally

After spawn, take stdout and stderr pipes and read them concurrently in bounded chunks. Do not call `.output()` or `.wait_with_output()` in governed production paths.

Each collector must:

1. append only while under its configured limit;
2. set a truncation/overflow state when the limit is reached;
3. continue draining without retaining bytes only when the child should be allowed to finish and pipe backpressure must be avoided;
4. terminate the process when the selected overflow policy requires it;
5. stop promptly on owner cancellation or timeout;
6. return a bounded result even when output is binary or invalid UTF-8.

Choose one default overflow behavior for ordinary tools and document any justified exception. A reasonable default is bounded retention plus continued drain until normal exit, with an optional terminate-on-overflow mode for machine-generated or adversarial jobs. Do not silently switch policies per caller.

### 6.2 Preserve useful diagnostics

A bounded prefix is simple but can lose the final error. A bounded head-plus-tail representation is acceptable if it remains deterministic and the rendered omission marker does not count as child output.

Do not introduce a spill-to-disk subsystem in this milestone. Existing job artifact storage may be used only when already part of the caller's contract and must still have a hard size bound.

### 6.3 Avoid collector deadlocks

Stdout and stderr must be read concurrently. Waiting for one stream before draining the other is prohibited. Collector tasks must be joined or aborted on every termination path.

## 7. Process-tree lifecycle design

### 7.1 Unix

Use a new process group or session for each finite governed execution, building on the existing `src/managed_process.rs` implementation.

Required lifecycle:

1. establish the child group/session without placing the daemon in it;
2. on ordinary exit, wait and reap the direct child;
3. on cancellation/timeout/overflow termination, send the documented graceful signal to the group when applicable;
4. wait for a short bounded grace period;
5. send forced termination to the group;
6. wait/reap the direct child;
7. record cleanup failure if signaling or reap fails unexpectedly.

Prevent PID/PGID reuse races by retaining direct child identity and minimizing delay between state observation and signaling. Do not signal negative IDs unless group creation is known to have succeeded.

### 7.2 Non-Unix

Preserve current supported behavior and compile cleanly. If Windows descendant cleanup is not currently a supported production contract, report direct-child-only behavior explicitly rather than claiming process-tree cleanup. Do not implement a partial Windows Job Object abstraction unless the repository already supports and tests Windows command execution.

### 7.3 Drop behavior

Drop remains a last-resort safety net, not the primary cleanup mechanism. Normal async ownership must explicitly cancel and await cleanup. A dropped handle may initiate kill, but tests and production callers should not rely on destructor timing for successful cleanup.

## 8. Expected production-code changes

Expected areas:

- `src/managed_process.rs` request/result and streaming collectors;
- `src/tool/bash.rs` shell/native execution adapters;
- `src/python_script/executor.rs` process launch adapter;
- `src/scheduler/executors.rs` where finite child execution duplicates the canonical service;
- execution ownership configuration/guard files;
- M001 sandbox helper integration beneath the canonical launch;
- focused process fixtures and tests;
- architecture execution documentation.

Potentially affected long-lived services must be inventoried, but migrate them only when their lifecycle fits this contract. Document exemptions with owner and reason in `docs/execution-ownership.toml` or the existing authoritative equivalent.

## 9. Storage, protocol, migration, and compatibility effects

Storage:

- no database migration expected;
- bounded output representation may affect stored job result payload size but must preserve existing durable-result schema or use backward-compatible optional metadata;
- do not rewrite historical outputs.

Protocol:

- daemon/frontend tool result APIs may gain optional termination/truncation/sandbox metadata;
- preserve existing human-readable stdout/stderr fields;
- avoid breaking serialized enum changes unless the current protocol already supports versioned additions.

Compatibility:

- preserve existing default timeouts and output rendering unless current behavior is unsafe;
- preserve raw shell availability and native/Python command semantics;
- invalid UTF-8 handling must remain deterministic;
- callers that previously received silently truncated output should now receive the same bounded text plus an explicit truncation indication.

## 10. Ordered work packages

### Work package A — Spawn inventory and ownership map

1. enumerate all production spawn sites;
2. classify lifecycle and owner;
3. identify all unbounded output paths;
4. identify all direct-child-only cancellation paths;
5. update the execution ownership map before broad migration so reviewers can detect missed paths.

### Work package B — Canonical request/result

1. consolidate executable, argv, cwd, env, timeout, cancellation, output, process-tree, sandbox, and provenance inputs;
2. consolidate termination and bounded-output result state;
3. add adapters for existing callers rather than cloning execution logic;
4. preserve existing public result rendering.

### Work package C — Bounded concurrent collectors

1. implement independent stdout/stderr limits;
2. implement deterministic overflow policy;
3. ensure continued draining cannot retain additional bytes;
4. integrate timeout and cancellation selection without races;
5. test binary and invalid UTF-8 output.

### Work package D — Process-tree cleanup

1. normalize Unix process-group/session creation;
2. centralize graceful/forced group termination and direct-child reap;
3. make cleanup idempotent;
4. distinguish cleanup failure from target failure;
5. retain explicit non-Unix limitation where necessary.

### Work package E — Caller migration

Migrate in this order:

1. raw shell in `src/tool/bash.rs`;
2. native execution in `src/tool/bash.rs`;
3. Python execution in `src/python_script/executor.rs`;
4. finite scheduler executors that duplicate the same lifecycle;
5. remaining finite governed command sites from the inventory.

Do not combine M003's routing/argv policy cleanup beyond passing existing typed argv into the canonical request.

### Work package F — Guards and documentation

1. update `docs/execution-ownership.toml` and `scripts/check_execution_ownership.py`;
2. reject new governed `.output()`/`.wait_with_output()` use;
3. document output-limit and process-tree semantics;
4. document any long-lived-service exemptions.

## 11. Focused verification

Create short, deterministic fixtures covering:

- stdout larger than the limit;
- stderr larger than the limit;
- simultaneous stdout/stderr pressure;
- invalid UTF-8;
- normal nonzero exit with retained diagnostics;
- timeout;
- explicit cancellation;
- child spawning a descendant that ignores a graceful signal;
- parent cancellation followed by descendant disappearance;
- sandbox-helper setup failure distinguished from target exit;
- no collector task leak after termination.

Expected command shape:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --test managed_process_descendants -- --test-threads=1
cargo test <managed output/termination target> -- --test-threads=1
cargo test <bash execution target> -- --test-threads=1
cargo test <python execution target> -- --test-threads=1
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
```

Use current target names. Tests that spawn descendants must have strict wall-clock timeouts and cleanup-on-failure logic.

## 12. Static guards

Extend the existing ownership guard rather than adding a new framework.

The guard should reject, within governed production modules:

- `.output()`;
- `.wait_with_output()`;
- direct `Command::new` that is not in the canonical executor, approved long-lived owner, sandbox helper bootstrap, or platform-specific documented exemption;
- process-global cwd mutation;
- new unowned `kill_on_drop` use as the sole cleanup policy.

Keep an explicit, small allow-list with path and reason. The guard must fail closed if its parser/matcher fails.

## 13. Acceptance criteria

M002 is complete only when:

- every inventoried finite governed spawn path uses the canonical process service;
- exemptions are lifecycle-specific, documented, and reviewed;
- stdout and stderr retention cannot exceed configured bounds by more than one fixed read chunk per implementation detail, and the returned buffers are strictly bounded;
- both streams are drained concurrently;
- truncation/overflow is explicit in results;
- timeout, cancellation, output termination, spawn failure, sandbox failure, target exit, and cleanup failure are distinguishable;
- Unix descendant fixtures are terminated and reaped under timeout/cancel paths;
- the daemon process group is never signaled;
- raw shell, native, Python, and scheduler authority semantics remain unchanged;
- M001 sandbox outcome is preserved;
- the execution ownership guard detects a temporary prohibited `.output()` fixture;
- focused tests and `scripts/verify.sh quick` pass;
- second review finds no unresolved critical/high/medium output or process-tree defect;
- no new CI lane, process-supervisor dependency, or unrelated scheduler redesign is introduced.

## 14. Stop conditions

Stop and report blocked when:

- M001 sandbox request/outcome contract is not closed;
- migration requires a breaking job/protocol schema change not covered by backward-compatible optional fields;
- a spawn site has durable lifecycle ownership that cannot be represented without redesigning its subsystem;
- Unix group cleanup exposes a scheduler-wide cancellation defect outside this milestone;
- current Windows support is ambiguous enough to require a separate platform decision;
- output artifact requirements imply a new storage subsystem.

Record the smallest follow-up. Do not broaden M002 into a complete scheduler or service-manager rewrite.

## 15. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/002-status.md` must include:

- accepted commit/PR;
- spawn-site inventory and final dispositions;
- canonical request/result summary;
- configured output limits and overflow behavior;
- focused large-output, dual-stream, timeout, cancellation, and descendant results;
- execution ownership guard negative proof;
- sandbox integration result;
- non-Unix limitation, if any;
- focused commands, quick verification, and hosted run reference;
- second-review findings by severity;
- confirmation that no governed unbounded output path remains.