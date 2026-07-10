# Phase 06: Capability Profiles and Enforced Python Sandboxing

## Objective

Move the Python scripting subsystem from policy-aware execution to policy-enforced execution. The current MVP has explicit modes, static risk analysis, environment clearing, workspace-root validation, pre/post snapshots, timeout handling, and capability denial. This phase should add a coherent capability-profile layer and enforce those profiles through platform sandboxing where available, with conservative fallback behavior where it is not.

The primary goal is to make the distinction between Analyze, Transform, and Verify authoritative at process-execution time rather than relying only on AST analysis and post-run snapshot detection.

## Scope

This phase covers:

- capability profile definitions;
- Python execution policy resolution;
- Landlock integration on supported Linux systems;
- portable fallback enforcement;
- subprocess supervision for Verify mode;
- environment, filesystem, and network policy;
- policy evidence in run results;
- focused validation.

This phase does not enable active command routing from BashTool. It strengthens the Python backend so later routing can safely depend on it.

## Existing substrate to reuse

Reuse rather than duplicate:

- `src/python_script/types.rs` execution modes and capability envelope;
- `src/python_script/analyze.rs` AST risk analysis;
- `src/python_script/sandbox.rs` compatibility checks;
- `src/python_script/executor.rs` cwd validation, env clearing, timeout, snapshots, and result assembly;
- existing BashTool Landlock integration;
- `crates/eggsentry` or other existing security primitives where applicable;
- session/worktree workspace-root authority;
- existing permission request types from command planning.

## Design principles

1. Profiles are explicit and serializable.
2. Static analysis informs policy but is not the sandbox boundary.
3. Unsupported enforcement should fail closed for capabilities that cannot be safely approximated.
4. Platform behavior must be reported in results.
5. Verify subprocess capability must be bounded, not equivalent to arbitrary shell access.
6. Transform write access is workspace-scoped and non-destructive by default.

## Workstream A: Define canonical Python capability profiles

Add a canonical profile type, for example:

```rust
pub struct PythonCapabilityProfile {
    pub mode: PythonExecutionMode,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
    pub allow_subprocess: bool,
    pub allowed_subprocesses: Vec<ExecutableRule>,
    pub allow_network: bool,
    pub allow_env: Vec<String>,
    pub allow_dependency_install: bool,
    pub allow_destructive_fs: bool,
    pub sandbox_requirement: SandboxRequirement,
}
```

Suggested profile semantics:

### Analyze

- read workspace;
- write nowhere except codegg-owned temporary run directory if required;
- no subprocess;
- no network;
- minimal env;
- no dependency installation;
- no destructive filesystem operations;
- sandbox required when platform support exists;
- snapshot verification remains defense-in-depth.

### Transform

- read workspace;
- write workspace;
- no outside-workspace access;
- no subprocess;
- no network;
- no dependency installation;
- no destructive filesystem mutation by default;
- all changes captured and diffed.

### Verify

- read workspace;
- no workspace writes;
- allow supervised subprocesses for known test/build commands;
- no arbitrary shell evaluation;
- no network;
- no dependency installation;
- snapshot verification remains mandatory.

## Workstream B: Add policy resolution

Introduce a policy-resolution step:

```text
PythonScriptRequest
  -> AST risk assessment
  -> requested mode
  -> workspace/session context
  -> permission state
  -> PythonCapabilityProfile
  -> enforcement backend
```

Required behavior:

- profile construction should be deterministic;
- risk analysis cannot silently widen a profile;
- explicit user/permission grants may only widen capabilities through typed permission paths;
- unsupported capabilities remain denied;
- policy resolution errors occur before script materialization or execution where possible.

Add a structured result:

```rust
pub struct PythonPolicyDecision {
    pub profile: PythonCapabilityProfile,
    pub denied: Vec<CapabilityViolation>,
    pub warnings: Vec<String>,
    pub enforcement_backend: SandboxBackend,
}
```

## Workstream C: Integrate Landlock on Linux

Reuse or extract the existing BashTool Landlock implementation into a shared execution sandbox module.

Required Landlock policy:

- workspace root read-only for Analyze and Verify;
- workspace root read-write for Transform;
- Python interpreter and required runtime/library paths readable/executable;
- codegg temporary script directory writable only as needed;
- `/proc`, `/sys`, device files, user home, SSH config, credential stores, and unrelated filesystem roots denied unless explicitly required;
- network denied through an available mechanism if Landlock alone cannot enforce it; otherwise report network isolation as unsupported and fail closed for network-capable scripts.

Do not duplicate low-level Landlock rule construction between BashTool and Python. Introduce a shared builder if practical.

## Workstream D: Portable fallback enforcement

On macOS, Windows, and Linux without usable Landlock:

- retain canonical cwd containment;
- retain env clearing;
- use isolated temporary directories;
- deny scripts whose AST risk requires capabilities that cannot be enforced;
- retain pre/post snapshots;
- reject outside-workspace paths where statically resolvable;
- report `SandboxBackend::PortableFallback` and its limitations;
- never claim OS-level sandboxing when only policy checks are active.

Consider optional platform-specific future hooks, but do not expand this phase into macOS Seatbelt or Windows AppContainer unless support already exists in the repo.

## Workstream E: Supervise Verify subprocesses

Verify mode should not receive unrestricted Python subprocess capability.

Add subprocess policy rules such as:

- allow direct argv execution only;
- reject `shell=True`;
- reject `os.system`, `os.popen`, and exec-family calls;
- permit known test/build binaries such as `cargo`, `pytest`, `python -m pytest`, `go test`, and configured project test commands;
- apply workspace cwd, env allowlist, timeout, output bounds, and process-tree termination;
- deny package managers in install mode;
- deny network-capable commands by default.

The AST scanner should collect enough call/constant information to identify obvious subprocess argv where possible. Unknown dynamic subprocess construction should be denied.

## Workstream F: Report enforcement evidence

Extend `PythonRunResult` or associated metadata with:

- effective profile identifier;
- sandbox backend;
- whether OS-level filesystem isolation was active;
- whether network isolation was active;
- allowed read/write roots;
- allowed subprocess rules;
- denied capability list;
- enforcement warnings.

Projection should remain compact but expose this information through structured metadata.

## Workstream G: Tests

Add unit tests for:

- each canonical profile;
- risk cannot widen capability profile;
- Analyze has no write roots except internal temp path;
- Transform writes only under workspace;
- Verify subprocess allowlist;
- Verify rejects shell execution and unknown subprocesses;
- network/dependency installation denied in all default profiles;
- destructive filesystem denied;
- explicit workspace root propagated to sandbox rules;
- enforcement metadata serialization.

Add Linux-gated integration tests for Landlock where CI supports it:

- Analyze cannot create a workspace file;
- Transform can modify workspace but not outside it;
- Verify can invoke an allowed test binary;
- access to a temp path outside allowed roots fails;
- home/SSH credential paths are inaccessible.

Portable fallback tests should prove failure-closed behavior for unsupported capabilities.

## Validation commands

```bash
cargo test -p codegg --lib python_script
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib permission
cargo test -p codegg --lib command_intent
```

Linux sandbox-specific tests:

```bash
cargo test -p codegg --features landlock python_script::sandbox
```

Full capped suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Acceptance criteria

- Analyze, Transform, and Verify resolve to explicit profiles.
- Profile enforcement occurs before execution.
- Linux uses shared Landlock enforcement where supported.
- Portable fallback is reported honestly and fails closed for unsupported capabilities.
- Verify subprocess capability is allowlisted and argv-based.
- Run results expose enforcement evidence.
- Existing Python MVP behavior remains compatible for safe scripts.
- Active BashTool routing remains deferred.
