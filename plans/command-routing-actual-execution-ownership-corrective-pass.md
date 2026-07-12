# Command Routing Actual-Execution Ownership Corrective Pass

## Objective

Correct the remaining persistence and provenance defects in command routing.

The current path derives RunStore ownership and `RunKind` from the planned routing decision even when execution actually occurred through raw shell because routing was disabled, Observe mode was active, validation failed, or structured dispatch fell back. This can cause missing records, incorrect run kinds, incorrect argv provenance, false delegated ownership, and premature model-safety flags.

The required invariant is:

> Every command execution produces exactly one canonical run record owned by the backend that actually executed it, with accurate invocation, fallback, and artifact-safety metadata.

## Non-goals

- Do not add command families.
- Do not enable active routing by default.
- Do not redesign the full RunStore schema.
- Do not broaden Python permissions.
- Do not add new TUI features.

## Workstream A: Add an authoritative execution outcome

Introduce a typed result representing actual execution rather than planned routing.

Suggested shape:

```rust
pub struct ExecutionOutcome {
    pub planned_backend: String,
    pub actual_backend: ActualExecutionBackend,
    pub ownership: RunOwnership,
    pub run_kind: RunKind,
    pub invocation: ActualInvocation,
    pub output: std::process::Output,
    pub fallback: Option<FallbackRecord>,
}

pub enum ActualExecutionBackend {
    RawShell,
    ManagedArgv,
    NativeTool,
    TestRunner,
    PythonScript,
}

pub enum ActualInvocation {
    Shell { shell: String, command: String },
    Argv { argv: Vec<String>, cwd: PathBuf },
    Python { mode: PythonExecutionMode, script_hash: String },
    Test { argv: Vec<String> },
}

pub struct FallbackRecord {
    pub from_backend: String,
    pub to_backend: String,
    pub reason: String,
}
```

Exact names may differ, but these semantics must remain explicit.

Rules:

1. Dispatch helpers return an execution outcome.
2. Persistence consumes the outcome.
3. Observe mode always reports actual backend `RawShell`.
4. A failed structured dispatch followed by shell fallback reports actual backend `RawShell`, ownership `Caller`, run kind `RawShell`, and a fallback record.
5. Planned and actual backend must be stored separately.

## Workstream B: Base persistence ownership on actual execution

Derive ownership from the actual executor:

- raw shell executed inside BashTool => `Caller`;
- managed argv executed inside BashTool => `Caller`;
- native tool executed inside BashTool => `Caller`;
- canonical TestRunner invocation => `DelegatedBackend`;
- canonical Python subsystem invocation => `DelegatedBackend`.

Required cases:

- Observe-mode test/Python/git/search commands produce one RawShell record.
- Active structured success produces one structured record.
- Structured failure followed by shell fallback produces one RawShell record.
- BashTool must not skip persistence merely because the planner selected TestRunner or Python.
- BashTool must not create a duplicate outer record when a delegated backend successfully owns the run.

## Workstream C: Route Python through the canonical Python subsystem

Replace direct `python3 -c` execution in BashTool with a typed call into the existing Python scripting service around `execute_python_script()` or `PythonScriptTool`.

Requirements:

- preserve Analyze/Transform/Verify mode;
- pass explicit workspace root and cwd;
- apply capability policy and sandbox selection;
- preserve snapshots, changed-file detection, diffs, and enforcement evidence;
- persist through the Python backend’s RunStore integration;
- return delegated ownership to BashTool;
- do not silently fall back to direct `python3 -c` without an explicit fallback decision.

Acceptance criteria:

- Active Python routing never skips Python policy enforcement.
- Structured Python success produces one Python run record.
- Python dispatch fallback is explicit and persisted as the backend actually used.

## Workstream D: Route tests through the canonical TestRunner

Ensure active test routing invokes the canonical TestRunner API rather than an ad hoc process path.

Requirements:

- use validated argv;
- preserve TestRunner timeout and report parsing;
- persist stdout, stderr, and structured report through RunStore;
- return delegated ownership;
- if dispatch fails before execution and raw fallback occurs, BashTool owns one RawShell record.

## Workstream E: Persist exact invocation provenance

Persist the invocation actually executed.

Raw shell:

```rust
argv = [shell_binary, "-c", original_command]
```

Managed/native execution:

```rust
argv = actual_argv
```

Python:

- store script hash and mode;
- do not represent it as shell argv unless shell actually ran it.

TestRunner:

- store validated actual argv and test scope/profile where available.

Rerun descriptors must be derived from actual invocation, not reconstructed from the original free-form string.

Acceptance criteria:

- direct argv execution is never recorded as fake `sh -c`;
- audit data distinguishes shell evaluation from direct process execution;
- rerun descriptors match the executable and arguments actually used.

## Workstream F: Persist planned-versus-actual backend metadata

Add backward-compatible optional fields to the run record or backend record:

```rust
pub planned_backend: Option<String>,
pub actual_backend: Option<String>,
pub fallback: Option<FallbackRecord>,
pub ownership: Option<RunOwnership>,
```

Use `#[serde(default)]` so old manifests remain readable.

Required evidence:

- Observe mode: planned structured backend, actual raw shell.
- Active success: planned and actual structured backend match.
- Fallback: planned structured backend, actual raw shell, reason present.

## Workstream G: Make raw artifact safety conservative

Raw stdout and stderr must be written with:

```rust
safe_for_model: false
```

Only a completed projection/redaction artifact may be marked model-safe.

Apply this consistently to:

- BashTool raw output;
- managed/native output;
- TestRunner raw logs;
- Python raw stdout/stderr.

Required flow:

1. Persist raw artifacts as not approved for model context.
2. Run projection and redaction.
3. Persist the final projection with model-safety approval only when policy succeeds.
4. Promotion must target approved projection artifacts or explicitly approved ranges.

## Workstream H: Persistence failure handling

Cover these failure modes:

- RunStore unavailable;
- `begin_run()` failure;
- artifact-write failure;
- `complete_run()` failure;
- structured dispatch failure before process creation.

Rules:

- persistence failure must not change the recorded actual backend;
- execution output remains valid even when persistence fails;
- incomplete records remain recoverable where supported;
- failures emit bounded diagnostics and metrics.

## Workstream I: Add focused ownership integration tests

Create a dedicated suite such as:

```text
tests/command_routing_execution_ownership.rs
```

Required matrix:

### Observe mode

- `cargo test` => one RawShell record;
- `python -c ...` => one RawShell record;
- `git status` => one RawShell record;
- `rg pattern src` => one RawShell record.

### Active structured success

- test => one Test record owned by TestRunner;
- Python => one Python record owned by Python subsystem;
- git read => one GitRead record with exact argv;
- search => one Search record with exact argv;
- managed build => one ManagedProcess record with exact argv.

### Active fallback

Inject dispatch failure for TestRunner, Python, native git, and managed search.

Assert:

- actual backend is RawShell;
- exactly one RawShell record exists;
- fallback metadata is present;
- no structured duplicate exists.

### Kill switch

Active config plus kill switch must produce one RawShell record while retaining the planned backend only as metadata.

### Artifact safety

- raw stdout/stderr are not approved for model context;
- final projected artifact may be approved after projection/redaction;
- promotion remains unavailable when only raw artifacts exist.

### Provenance

- direct argv routes preserve exact argv;
- raw shell routes preserve shell invocation;
- no fake `sh -c` for direct execution.

## Workstream J: Correct validation evidence

Update:

```text
docs/validation/command-routing-final-closure.md
```

Requirements:

1. Record the final implementation commit SHA, not the plan SHA.
2. Separate passed, failed, timed-out, and skipped checks.
3. Include the new ownership integration suite.
4. Do not claim full closure if ownership or RunStore tests time out.
5. Record platform, architecture, Rust version, and sandbox backend accurately.
6. Note when GitHub combined status is unavailable.

## Validation commands

```bash
cargo test -p codegg --lib tool::bash
cargo test -p codegg --lib command_routing
cargo test -p codegg --lib command_intent::plan
cargo test -p codegg --lib python_script
cargo test -p codegg --lib test_runner
cargo test -p codegg-core run_store
cargo test --test command_routing_execution_ownership
cargo test --test command_routing_adversarial
cargo test --test python_sandbox_adversarial
cargo test --test context_projection_adversarial
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

If the full suite exceeds the available execution window, record that honestly; all ownership-specific tests must still complete.

## Recommended implementation order

1. Add execution outcome and actual invocation types.
2. Refactor dispatch helpers to return outcomes.
3. Correct Observe and fallback persistence.
4. Route TestRunner through its canonical API.
5. Route Python through its canonical API.
6. Persist exact invocation and fallback metadata.
7. Set raw artifacts to not approved for model context.
8. Add ownership and failure-injection tests.
9. Correct and rerun validation evidence.

## Closure criteria

This pass is complete when:

- persistence follows actual execution rather than planned routing;
- Observe-mode test/Python/git/search commands produce one RawShell record;
- structured fallback produces one RawShell record with fallback evidence;
- structured Python uses the canonical Python policy/sandbox path;
- structured tests use the canonical TestRunner path;
- direct execution preserves exact argv;
- raw stdout/stderr are not approved for model context;
- one command produces exactly one canonical run record;
- ownership integration tests pass;
- validation evidence references the actual implementation commit and does not overstate timed-out checks.
