# Phase 05: First-Class Python Scripting MVP

## Objective

Add a first-class Python scripting subsystem on top of the command intent, planning, projection, permission, and RTK-aware output infrastructure. Python should become an explicit, inspectable, reversible, and context-efficient execution family rather than an opaque `bash` escape hatch.

This phase should implement the MVP for `Analyze`, `Transform`, and `Verify` Python modes. It should route recognized Python commands/scripts into the subsystem where possible, preserve raw artifacts, capture changed files, and project results safely.

## Motivation

LLMs are highly proficient at Python and often use it to recover from tool friction, perform bulk transformations, inspect data, parse logs, generate fixtures, and work around harness limitations. In codegg, this behavior should not be treated as accidental misuse. It should be formalized with mode-specific policy, static risk analysis, sandbox/snapshot integration, durable provenance, and structured projection.

The goal is not to replace native tools. The goal is to make Python the safe long-tail composition layer while native tools remain preferred for stable first-class operations.

## Execution modes

Add `PythonExecutionMode`:

```rust
pub enum PythonExecutionMode {
    Analyze,
    Transform,
    Verify,
}
```

Defer `Privileged` mode to later hardening phases.

### Analyze

Default mode. The script may read workspace files and produce stdout/stderr/artifacts. It may not write workspace files, spawn subprocesses, access network, install dependencies, or read outside the workspace unless policy explicitly allows.

Typical uses:

- parse source files;
- inspect TOML/JSON/YAML;
- summarize test logs;
- compute dependency graphs;
- generate reports to stdout;
- analyze diffs or file contents.

### Transform

Workspace-limited write mode. The script may write under the workspace after permission checks. codegg must snapshot or otherwise record pre-run state, execute the script, compute changed files, capture a diff, and project the diff. Writes outside workspace are denied.

Typical uses:

- bulk text rewrites;
- generated fixture updates;
- mechanical migrations;
- code/documentation normalization;
- safe file generation under known directories.

### Verify

Read mode plus controlled subprocess capability. The script may run project-local checks or helper commands through a supervised subprocess mechanism, not arbitrary unlogged process spawning. Subprocess invocations must be captured as events and surfaced in the run report.

Typical uses:

- small verification harnesses;
- invoking project binaries;
- running narrow tests from Python;
- comparing generated output against expected data.

## Proposed module layout

Add a new subsystem rather than burying Python inside `BashTool`:

```text
src/python_script/
  mod.rs
  types.rs
  analyze.rs        # static risk analysis
  executor.rs       # materialize and execute scripts
  sandbox.rs        # mode/capability envelope and platform policy
  snapshot.rs       # pre/post snapshot and changed-file capture adapters
  projection.rs     # PythonRun -> ProjectionResult
  tool.rs           # model-facing PythonScriptTool
  route.rs          # CommandIntent/CommandPlan adapters
```

If a shorter path is preferred, use `src/tool/python_script.rs` for the model-facing tool and keep the subsystem in `src/python_script/`.

## Core request/result types

```rust
pub struct PythonScriptRequest {
    pub code: String,
    pub mode: PythonExecutionMode,
    pub cwd: PathBuf,
    pub timeout_secs: Option<u64>,
    pub capture_policy: CapturePolicy,
    pub context_policy: ContextPolicy,
    pub expected_outputs: Vec<PythonExpectedOutput>,
    pub session_id: Option<String>,
}

pub struct PythonRunEntry {
    pub id: CommandRunId,
    pub script_hash: String,
    pub mode: PythonExecutionMode,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit: CommandExit,
    pub risk: PythonRiskAssessment,
    pub capabilities: PythonCapabilityEnvelope,
    pub stdout: OutputHandle,
    pub stderr: OutputHandle,
    pub script_body: OutputHandle,
    pub changed_files: Vec<PathBuf>,
    pub diff: Option<OutputHandle>,
    pub subprocesses: Vec<PythonSubprocessEvent>,
    pub artifacts: Vec<ArtifactHandle>,
}
```

Keep field names aligned with existing shell/test types where possible.

## Static risk analysis MVP

Implement a Python risk analyzer before execution. The MVP can invoke a tiny internal Python AST scanner script, because Python's standard `ast` module is reliable and avoids premature Rust parser dependency selection. If invoking Python for scanning is undesirable, add a small Rust-side scanner later.

The analyzer should parse script code and report:

- imports;
- suspicious imports;
- direct calls of interest;
- file write attempts detectable statically;
- subprocess/network/dependency/env/dynamic execution indicators;
- use of `eval`, `exec`, `compile`, `__import__`;
- use of `pickle`, `marshal`, `ctypes`;
- use of `subprocess`, `os.system`, `pty`, `socket`, `urllib`, `requests`, `httpx`;
- use of package managers through subprocess-like patterns;
- deletion or permission-changing calls such as `shutil.rmtree`, `Path.unlink`, `os.remove`, `chmod`, `chown`.

Static analysis must not be treated as a proof of safety. It feeds the capability envelope and permission prompts. Runtime sandbox/snapshot checks remain required.

## Capability envelope

Add explicit capabilities:

```rust
pub struct PythonCapabilityEnvelope {
    pub read_workspace: bool,
    pub write_workspace: bool,
    pub read_outside_workspace: bool,
    pub write_outside_workspace: bool,
    pub subprocess: bool,
    pub network: bool,
    pub env_access: bool,
    pub dependency_install: bool,
    pub destructive_fs: bool,
}
```

Default envelopes:

- `Analyze`: read workspace only.
- `Transform`: read workspace and write workspace.
- `Verify`: read workspace and supervised subprocess.

Network, dependency installation, outside-workspace access, and destructive filesystem operations should be denied or ask-only in later phases. For the MVP, prefer denial unless there is a clear repo-local need.

## Executor behavior

The executor should:

1. validate request and cwd;
2. run static risk analysis;
3. derive capability envelope from mode plus risk;
4. perform permission checks;
5. materialize script to a controlled temp directory;
6. clear environment and restore only minimal development vars required by policy;
7. execute `python3` or configured interpreter with timeout;
8. capture stdout/stderr to output handles;
9. for `Transform`, capture pre/post file state and diff;
10. build `PythonRunEntry`;
11. project result through the unified projection pipeline;
12. return bounded model-facing result plus handles.

The model should not need to manually create temp files for one-off scripts.

## Interpreter selection

Initial interpreter resolution:

1. If `VIRTUAL_ENV` is set and contains a Python executable, use that.
2. Else use `python3` from PATH.
3. Else use `python` from PATH if available.
4. Else return a clear execution error.

Record interpreter path/version in the run entry if practical.

Do not install Python or dependencies automatically.

## Filesystem and snapshot behavior

For `Analyze`, detect changed files after execution. If any workspace file changed, mark the run as policy violation or failed. Do not silently accept writes in analyze mode.

For `Transform`:

- capture pre-run worktree snapshot or file state summary;
- execute script;
- compute changed files;
- generate diff handle;
- project changed-file summary and selected diff hunks;
- allow later rollback through existing snapshot mechanisms if available.

For `Verify`, treat writes as denied unless explicitly allowed by future policy.

## Subprocess handling

The MVP cannot fully intercept all Python subprocess creation without deeper sandboxing. Therefore:

- static analysis should flag subprocess usage;
- `Analyze` and `Transform` should deny scripts with subprocess indicators by default;
- `Verify` may allow subprocess only if codegg can supervise or if the script is executed under a mode that explicitly permits it;
- subprocess use must be recorded in `PythonRunEntry` when statically detected and, if possible, dynamically logged by requiring helper wrappers in later phases.

Do not claim perfect subprocess mediation in this phase.

## Network and dependency policy

MVP default:

- network denied;
- dependency installation denied;
- outside-workspace writes denied;
- outside-workspace reads denied unless explicitly required for interpreter/runtime internals.

If a script imports `requests`, `httpx`, `urllib`, or `socket`, mark network risk and require denial/ask according to policy. For this phase, prefer denial to reduce scope.

## Model-facing tool

Add `PythonScriptTool` with a schema similar to:

```json
{
  "type": "object",
  "properties": {
    "code": { "type": "string" },
    "mode": { "type": "string", "enum": ["analyze", "transform", "verify"] },
    "workdir": { "type": "string" },
    "timeout": { "type": "number" },
    "intent": { "type": "string" }
  },
  "required": ["code", "mode"]
}
```

Tool description should tell models to use the least-powerful mode and prefer native tools for simple read/write/edit/test/git operations. Python is for compositional analysis, bulk transforms, and custom verification.

## Routing Python commands from shell intent

Update command routing so recognized Python shell commands can map to the Python subsystem when routing is enabled:

- `python script.py` and `python3 script.py` become Python script file execution requests if the file is under workspace.
- `python -m pytest` remains test intent.
- `python -c ...` becomes PythonScriptTool-style execution if the code can be extracted safely; otherwise complex shell fallback or rejection.
- heredoc Python scripts can be recognized later; if implemented in MVP, preserve exact script body and route through Python subsystem.

Do not simply remove `python -c` from bash blocked patterns. Route it away from bash when safe; otherwise keep existing bash protection.

## Projection

Python projection should emit:

- run id;
- mode;
- status/exit code/duration;
- risk flags;
- imports of interest;
- stdout summary and handle;
- stderr summary and handle;
- changed files;
- diff summary and handle for transforms;
- subprocess/network/dependency warnings;
- RTK metadata when compression is used;
- exact preserved spans for file paths, error lines, and diff hunks.

Large stdout/stderr/diffs should be RTK eligible if RTK is enabled. Exact error/file/diff spans must remain preserved.

## TUI and artifact visibility

The MVP can initially return model-facing text through the tool result, but it should prepare metadata for a first-class TUI cell later. If easy, render Python runs similarly to shell cells with mode/risk/status/changed-files visible.

At minimum, raw script body, stdout, stderr, and diff handles must be available for inspection.

## Tests

Add tests for:

- `Analyze` script with no writes succeeds;
- `Analyze` script attempting workspace write fails or is flagged;
- `Transform` script writing a workspace file captures changed file and diff;
- `Transform` script writing outside workspace is denied;
- `Verify` script with allowed subprocess path is handled according to policy;
- network imports are flagged;
- dependency install attempts are flagged/denied;
- `eval`/`exec` are flagged;
- stdout/stderr truncation and output handles work;
- RTK unavailable fallback works;
- Python command intent routes `python -m pytest` to test runner, not PythonScriptTool;
- `python -c` does not execute through raw bash when Python routing is enabled.

Use temp directories for filesystem mutation tests. Avoid relying on global Python packages beyond stdlib.

## Acceptance criteria

- `PythonScriptTool` exists and is registered where appropriate.
- `Analyze`, `Transform`, and `Verify` modes are represented and enforced at least at the policy/snapshot level.
- Python scripts are materialized and executed as controlled artifacts, not opaque shell strings.
- Static risk analysis flags imports/calls of interest.
- Transform mode captures changed files and diff handles.
- Analyze mode does not silently allow writes.
- Python stdout/stderr enter context through projection, not unbounded raw dumps.
- RTK eligibility is applied to large Python outputs without requiring RTK.
- Existing bash/test/git routing behavior remains stable.

## Suggested validation commands

```bash
cargo test -p codegg --lib python_script
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib test_runner
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
```

If Python plugin/example surfaces are touched:

```bash
PYTHONPATH=examples/plugins/sdk-python \
  python3 -m unittest discover examples/plugins/sdk-python/tests -v
```

Broader fallback:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Risks and mitigations

The main risk is overclaiming sandbox strength. Be explicit in docs and code comments: MVP static analysis and snapshot checks are not a perfect sandbox. On Linux, integrate Landlock where available. On other platforms, use minimal env, workspace checks, temp materialization, and snapshot/diff enforcement.

The second risk is making Python too constrained, causing models to flee back to raw bash. Mitigate by making the safe path ergonomic: direct `code` input, clear mode choices, automatic temp script materialization, useful stdout/stderr/diff projections, and low friction for read-only analysis.
