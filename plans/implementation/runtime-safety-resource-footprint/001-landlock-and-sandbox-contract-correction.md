# Runtime Safety, Resource Control, and Footprint Milestone 001 — Landlock and Sandbox Contract Correction

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 001

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Relevant long-term requirements:

- daemon-owned execution authority remains authoritative;
- tool and child-process authority must not exceed the originating request;
- local execution must fail predictably and remain observable;
- security claims must match enforced behavior.

Related ADRs:

- None required if the implementation replaces the custom backend while preserving the existing sandbox capability and public execution contracts.
- Stop and propose an ADR only if implementation requires changing daemon ownership, public CLI behavior, or the external process protocol.

Primary class: security invariant and correctness

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/001-status.md`

Independent closure review: required

## 1. Objective

Replace the current handwritten Linux Landlock implementation with a maintained enforcement path, make sandbox requirement and outcome states explicit, remove non-async-signal-safe policy construction from `pre_exec`, and ensure Python and other current sandbox consumers receive the policy they requested.

The accepted result must distinguish:

```text
required + enforced      -> execute and report enforced backend/ABI
required + unavailable   -> do not execute; return sandbox-unavailable failure
required + setup error   -> do not execute; return sandbox-setup failure
best effort + enforced   -> execute and report enforced backend/ABI
best effort + unavailable/error -> use named portable fallback only when permitted,
                                   and report the fallback rather than Landlock
sandbox disabled         -> execute without claiming confinement
```

The milestone is complete only when a successful Landlock result means the ruleset was fully applied to the child process before target execution.

## 2. Explicit non-goals

This milestone must not:

- redesign scheduler, job, Tool Programs, or approval authority;
- implement seccomp, namespaces, containers, chroot, macOS Seatbelt, Windows Job Objects, or a general cross-platform sandbox framework;
- attempt to sandbox the long-running CodeGG daemon process itself;
- apply Landlock to the parent and then try to undo it;
- retain a parallel handwritten Landlock syscall implementation as a fallback;
- add a broad security test matrix or privileged CI runner;
- treat an unsupported kernel as proof that the implementation works;
- silently continue unsandboxed when the request requires enforcement;
- change raw shell approval policy or expand command authority;
- perform unrelated Python executor or managed-process refactors assigned to M002.

## 3. Current implementation evidence

Inspect at minimum:

- `src/security/sandbox.rs`;
- `src/python_script/executor.rs`;
- the Python policy/request types adjacent to `src/python_script/`;
- every production call to the current sandbox availability, configuration, or enforcement API;
- tests that mention Landlock, sandbox mode, Python read-only mode, or transform mode;
- architecture documentation that describes sandbox behavior.

The reviewed baseline contains the following defects or unsafe assumptions:

1. `src/security/sandbox.rs` encodes Landlock syscall numbers and access-right bits directly.
2. Availability detection uses host files rather than the authoritative Landlock ABI query and does not prove that `restrict_self` can succeed.
3. Child setup does not reliably establish all Landlock prerequisites before enforcement.
4. rule-add errors may be logged and ignored, allowing a partial ruleset to be described as enforced;
5. deny paths are represented as zero-access rules even though Landlock is fundamentally an allow-list mechanism;
6. explicit sandbox requests can degrade through warning-only behavior;
7. `src/python_script/executor.rs` performs policy construction, formatting/logging, allocation, and other Rust work inside `pre_exec`;
8. Python transform execution currently selects a read-only sandbox mode even when the transform contract requires controlled workspace writes;
9. command output capture remains unbounded, but that is owned by M002 except where a small change is required to expose sandbox-helper failures correctly.

The implementation agent must confirm each observation against current code before editing. If main has changed, record the adjusted evidence in the closure record.

## 4. Invariants that cannot regress

- The daemon remains unrestricted by the child sandbox and continues to manage other projects and jobs.
- Sandbox configuration is derived from typed workspace and execution context, not ambient process cwd.
- Child authority is no broader than the originating tool/Python request.
- A required sandbox failure prevents target code from running.
- A best-effort fallback is explicitly represented in the result and logs.
- The target executable and required runtime libraries remain readable/executable under Landlock.
- Read-only mode cannot write the workspace, temporary snapshots, or explicitly protected paths except for narrowly owned runtime files required by the execution contract.
- Transform/workspace-write mode can write only approved workspace/output paths, not arbitrary filesystem locations.
- Rule construction is all-or-nothing. Any required rule failure aborts launch.
- No complex Rust code executes in an unsafe post-fork `pre_exec` context.
- Unsupported kernels produce a stable unavailable result rather than a false enforced result.

## 5. Required design

### 5.1 Typed sandbox request and outcome

Introduce or normalize types equivalent to:

```rust
enum SandboxRequirement {
    Disabled,
    BestEffort,
    Required,
}

enum SandboxProfile {
    ReadOnlyWorkspace,
    WorkspaceWrite,
}

enum SandboxBackend {
    Landlock { abi: u32 },
    PortableProcessOnly,
    None,
}

enum SandboxOutcome {
    Enforced(SandboxBackend),
    Fallback { backend: SandboxBackend, reason: String },
    Disabled,
}
```

Exact names may follow current terminology. Do not expose free-form strings as the authoritative state.

Errors must distinguish at least:

- unsupported/unavailable backend;
- invalid path or policy construction;
- backend setup/restriction failure;
- helper protocol/launch failure.

The execution result may render these as user-facing diagnostics, but typed state must exist beneath the rendering.

### 5.2 Maintained Landlock backend

Prefer the maintained Rust `landlock` crate with default features disabled unless a required feature is documented. Pin a compatible version through Cargo.lock and use the crate's ABI-aware access-right selection rather than hard-coded numeric constants.

The backend must:

1. query the actual supported Landlock ABI;
2. select handled filesystem rights supported by that ABI;
3. construct an allow-list ruleset for required runtime and workspace paths;
4. add every required rule, treating any failure as policy failure;
5. establish `no_new_privs`/restriction preconditions through the maintained API;
6. restrict only the short-lived child/helper process;
7. report the actual ABI and backend obtained.

Remove the current direct syscall definitions and custom access-right bit arithmetic from production code.

### 5.3 Safe launch boundary

Do not construct or apply a maintained Landlock ruleset from a closure that performs general Rust allocation after fork.

Use the smallest coherent safe architecture. Preferred approach:

- add a private one-shot sandbox launch helper mode to the existing executable or a tiny private helper binary;
- the daemon/parent serializes a typed launch specification through an inherited pipe, anonymous temporary file, or similarly bounded local channel;
- the helper starts as a normal process, validates and parses the specification, constructs and applies Landlock in its own process, then replaces itself with the target through `exec`;
- target stdout, stderr, stdin, process-group, timeout, and cancellation remain owned by the parent execution service;
- helper setup failures exit through reserved typed status/reporting rather than being confused with target exit codes.

The helper must not become a new daemon, network service, or public API. It must be one-shot and inherit no greater authority than the parent request.

An alternative is acceptable only if it demonstrates that all post-fork operations are async-signal-safe and does not retain complex Rust policy construction in `pre_exec`.

### 5.4 Allow-list construction

Build the minimum practical read/execute allow-list required for the selected target and platform. Derive paths from current execution context and executable resolution rather than assuming one distribution layout.

At minimum evaluate:

- target executable and interpreter path;
- dynamic loader and runtime library roots required by the host distribution;
- workspace/project root;
- approved temporary directory or script/snapshot path;
- read-only configuration/model assets required by the child;
- `/dev/null` and similarly required process devices when applicable;
- explicit writable output paths for transform mode.

Do not emulate deny rules with empty access masks. Paths outside the allow-list are denied by the handled rights.

Canonicalize and validate paths before helper launch. Missing optional paths may be omitted; missing required paths must fail policy construction.

### 5.5 Python mode correction

Map Python execution modes to the typed profile intentionally:

- inspect/read-only analysis -> `ReadOnlyWorkspace`;
- transform that writes approved output/workspace files -> `WorkspaceWrite` with only the required writable roots;
- any mode requiring no sandbox -> explicit disabled or best-effort policy according to existing user configuration.

Do not hard-code every Python request to read-only. Preserve snapshot/restore behavior only where it remains necessary after real enforcement.

## 6. Expected production-code changes

Expected areas include:

- `src/security/sandbox.rs` or a replacement module split into policy, backend, and helper protocol concerns;
- root command/bootstrap dispatch for a private sandbox helper mode;
- `src/python_script/executor.rs` sandbox request construction and result propagation;
- shared execution result/error types needed to carry sandbox outcome;
- Cargo manifests for the maintained Landlock dependency;
- architecture/security documentation and operator diagnostics;
- focused tests/fixtures.

Avoid changing unrelated tool policy, provider, scheduler, database, or frontend code.

## 7. Storage, protocol, migration, and compatibility effects

Storage:

- no database migration;
- no durable schema change;
- helper specifications are ephemeral and must be deleted/closed on completion or cancellation.

Protocol:

- no daemon network/IPC breaking change;
- internal execution results may gain backward-compatible sandbox outcome fields;
- private helper protocol is local-only, versioned, bounded in size, and not a public compatibility promise.

Compatibility:

- Linux hosts without Landlock remain supported through the existing portable behavior only for disabled/best-effort requests;
- required mode must return a clear error on unsupported hosts;
- non-Linux builds must compile without the Landlock dependency path and report the correct backend availability;
- existing configuration values should map to the new requirement/profile types without silent semantic broadening.

## 8. Ordered work packages

### Work package A — Inventory and contract tests

1. Find every sandbox API caller and every user/config option that selects sandbox behavior.
2. Add focused unit tests for required, best-effort, and disabled outcome selection before replacing the backend.
3. Add tests proving Python modes map to the intended profile.
4. Document current fallback behavior that will intentionally change.

### Work package B — Typed policy and outcome

1. Introduce the typed requirement/profile/outcome/error model.
2. Move path collection and policy validation into parent-side pure code.
3. Make partial rule construction impossible to represent as enforced.
4. Update diagnostics to report requested and obtained state.

### Work package C — Maintained backend and helper

1. Add the maintained Landlock dependency under Linux target configuration.
2. Implement authoritative ABI query and ABI-aware rights.
3. Implement the private one-shot helper protocol and dispatch.
4. Apply the complete ruleset in the helper, then `exec` the target.
5. Reserve and decode helper setup failures separately from target failures.
6. Remove handwritten syscalls and custom availability heuristics.

### Work package D — Python integration

1. Replace direct `pre_exec` policy construction with the helper-backed launch path.
2. Correct read-only versus transform/write profile selection.
3. Preserve timeout, stdin, environment, cwd, and snapshot semantics.
4. Ensure sandbox failure occurs before user Python code runs.

### Work package E — Focused Linux enforcement tests

Add bounded integration fixtures that:

- read an allowed workspace file;
- fail to modify the workspace in read-only mode;
- write an approved file in workspace-write mode;
- fail to write outside approved roots;
- execute a normal interpreter/runtime dependency;
- return unavailable with a reason on unsupported kernels;
- prove required mode does not execute a marker command when setup fails;
- prove a missing required rule aborts setup rather than producing partial enforcement.

Tests must detect host support at runtime and emit a precise skip reason. A skip is not closure evidence for enforcement; the closure record must include one supported-Linux run or explicitly remain blocked for security closure.

### Work package F — Documentation and cleanup

1. Update architecture/security documentation with the exact requirement/outcome contract.
2. Document non-Linux and unsupported-kernel behavior.
3. Remove stale comments claiming enforcement from the old backend.
4. Add a short maintainer note describing how to run the supported-host focused test.

## 9. Focused verification

Run the narrowest current commands that cover the implementation. Expected command shape:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test <sandbox unit target> -- --test-threads=1
cargo test <python sandbox/profile target> -- --test-threads=1
cargo test <linux landlock integration target> -- --test-threads=1
scripts/verify.sh quick
```

Use actual target names created or already present; do not add duplicate targets solely to match this example.

On one Landlock-capable Linux host, record:

- kernel version;
- reported Landlock ABI;
- enforcement test result;
- required-mode setup-failure result;
- read-only and workspace-write results.

Do not add this host-specific run as a new permanent CI matrix.

## 10. Static guards

Add only proportionate guards:

- a source guard may reject direct `landlock_create_ruleset`, `landlock_add_rule`, or hard-coded syscall-number definitions outside an explicitly approved backend test fixture;
- a source guard may reject new sandbox `pre_exec` closures in governed modules;
- do not create a general unsafe-code scanner or dependency-policy framework.

Integrate guards into the existing quick/hosted path only when they are cheap and deterministic.

## 11. Acceptance criteria

M001 is complete only when:

- the custom Landlock syscall implementation is removed from production code;
- authoritative ABI detection replaces filesystem heuristics;
- all required rules must succeed before enforcement is reported;
- no zero-access pseudo-deny rule remains;
- required mode cannot continue through an unavailable or failed backend;
- best-effort fallback is typed and visible;
- complex policy construction no longer occurs in `pre_exec`;
- the daemon itself is not Landlock-restricted;
- Python read-only and transform/write modes request and receive distinct intended profiles;
- target execution cannot begin after helper setup failure;
- focused supported-Linux enforcement evidence passes;
- non-Linux and unsupported-Linux behavior compile and report correctly;
- no new daemon, network protocol, CI matrix, or unrelated security framework is introduced;
- `scripts/verify.sh quick` passes on the accepted revision;
- independent review finds no unresolved critical/high/medium sandbox defect.

## 12. Stop conditions

Stop and report blocked rather than improvising when:

- the maintained Landlock crate cannot support the required ABI/path rule semantics;
- the only proposed approach requires applying Landlock to the daemon process;
- helper launch cannot preserve existing process ownership and cancellation without first completing M002;
- current configuration semantics are ambiguous enough to require a public compatibility decision;
- no Landlock-capable host is available for required closure evidence;
- implementation exposes a broader scheduler/tool authority defect outside this milestone.

A blocker may produce one narrow corrective or prerequisite plan. Do not expand M001 into seccomp, containerization, or scheduler redesign.

## 13. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/001-status.md` must include:

- accepted commit/PR;
- removed custom backend files/symbols;
- final typed sandbox contract;
- supported-Linux kernel and ABI evidence;
- read-only, workspace-write, outside-root denial, and setup-failure results;
- non-Linux/unsupported behavior evidence;
- focused commands and outcomes;
- `scripts/verify.sh quick` and hosted `verify` reference where available;
- independent reviewer findings by severity;
- compatibility/configuration disposition;
- explicit confirmation that required mode does not fail open.
