# Phase 02: Command Planner and Backend Routing Skeleton

## Objective

Introduce a planning layer that maps `CommandIntent` into an inspectable `CommandPlan` without broadly changing behavior. The planner should decide which backend would execute an intent, which permissions are required, which projector should handle the result, and whether RTK is eligible. In this phase, routing should remain disabled or opt-in for most paths; the primary goal is to create the stable seam that later phases will use.

## Relationship to Phase 01

Phase 01 creates command source, intent, shell-shape, and risk classification. Phase 02 consumes that result and produces a concrete execution plan. The planner must not reparse raw shell independently except for defense-in-depth validation. It should rely on `CommandIntent` and existing subsystem APIs.

## Proposed module layout

Extend the command intent module or create a sibling execution planning module:

```text
src/command_intent/
  plan.rs
  backend.rs
  permissions.rs
  route.rs
```

Alternatively:

```text
src/execution/
  intent/
  plan.rs
  backend.rs
  permissions.rs
```

Prefer a layout that can later host Python and non-shell execution without coupling everything to `src/shell/`.

## Core types

Add `CommandPlan` and backend route types.

```rust
pub struct CommandPlan {
    pub intent: CommandIntent,
    pub backend: ExecutionBackend,
    pub permission_requests: Vec<CommandPermissionRequest>,
    pub projector: ProjectorRoute,
    pub rtk_policy: RtkProjectionPolicy,
    pub context_policy: ContextPolicy,
    pub notes: Vec<String>,
}

pub enum ExecutionBackend {
    RawShell { command: String },
    ManagedArgv { argv: Vec<String>, cwd: PathBuf },
    NativeTool { route: ToolRoute },
    TestRunner { request: TestRunRequest },
    PythonScript { request: PythonScriptRequestStub },
    Reject { reason: String },
}

pub enum ProjectorRoute {
    Raw,
    Truncated,
    ErrorRetention,
    GitStatus,
    GitDiff,
    GitLog,
    TestReport,
    FileSearch,
    PythonRun,
    RtkEligible(Box<ProjectorRoute>),
}
```

For Phase 02, `PythonScriptRequestStub` can be a placeholder that carries code/source/mode guess but is not executable yet.

## Permission planning

Add a command-level permission request model that can later bridge into the existing permission system.

```rust
pub struct CommandPermissionRequest {
    pub capability: ExecutionCapability,
    pub path: Option<PathBuf>,
    pub risk_level: RiskLevel,
    pub reason: String,
    pub default_decision: PermissionDefault,
}

pub enum PermissionDefault {
    Allow,
    Ask,
    Deny,
}
```

Initial default decisions:

- Low-risk read-only workspace commands: allow.
- Test runner commands matching strict validated argv forms: allow or ask according to existing tool policy.
- Git read: allow.
- Git write: ask.
- Workspace writes: ask.
- Delete/destructive file operations: ask or deny depending on severity.
- Network: ask or deny.
- Dependency installation: ask.
- Outside-workspace access: ask or deny.
- Complex shell with elevated risk: ask.
- Known blocked/destructive shell patterns: reject.

Do not bypass existing `PermissionChecker`. Phase 02 should prepare requests and optionally log them. Later phases can bridge these requests into existing `PermissionPending` flows.

## Backend planning rules

Implement planning rules as a deterministic rule table. Avoid model-dependent routing.

### Test commands

For `CommandIntentKind::TestRun`, prepare `ExecutionBackend::TestRunner` when the command can map into `TestRunRequest` or validated custom command form. Otherwise plan `ManagedArgv` or `RawShell` with `TestReport`/`ErrorRetention` projector.

The planner should reuse existing `test_runner::custom` validation for custom commands. Do not accept raw shell syntax into test runner routes.

### Git read

For `GitRead` intents, prefer native `egggit`/tool routes when supported. If a command has unusual but safe flags not yet supported by native code, use `ManagedArgv` or `RawShell` with a git projector.

### Git write

For `GitWrite`, do not route silently. Produce an explicit plan requiring permission. Backend can remain `RawShell` initially. Later phases can route some operations to native git write tools if they exist.

### File search/list/read

For `rg`, `fd`, `find`, `ls`, `pwd`, `cat`, and safe `sed -n`, prefer existing native read/glob/grep tools when the mapping is exact. Otherwise use `ManagedArgv` with file-search/listing projection.

### Python

For Python-like commands, produce `ExecutionBackend::PythonScript` only as a non-executable stub in Phase 02. If execution is requested before Phase 05, fallback remains existing bash behavior. Preserve the plan output so tests can assert future routing.

### Unknown/complex shell

Use `RawShell` with the existing bash security policy. If Phase 01 risk classification marks the command destructive, planning may return `Reject` before raw shell.

## RTK policy skeleton

Add the policy type now, but do not require RTK to be installed.

```rust
pub enum RtkProjectionPolicy {
    Disabled,
    Eligible {
        min_raw_bytes: usize,
        preserve_exact_spans: Vec<ProjectionSpanKind>,
        goal: CompressionGoal,
    },
    RequiredForPromotion,
}
```

Initial rules:

- Disabled for short read-only outputs.
- Eligible for raw shell, complex shell, long git diffs, long test logs, long search output, and future Python stdout/stderr.
- Preserve exact compiler errors, test failure names, file paths, line numbers, and diff hunks selected by projectors.

Phase 02 should only attach policy metadata. The actual RTK projection behavior is Phase 03.

## Execution neutrality

This phase should avoid destabilizing the current repo. Add a feature flag or config toggle such as:

```toml
[command_intent]
enabled = true
route_execution = false
log_plans = true
```

If config schema churn is undesirable in Phase 02, keep this behind internal functions and tests only. The important constraint is that planner output must be testable without changing agent behavior.

## Integration touch points

- `src/tool/bash.rs`: after Phase 01 classification, call planner only for tracing/test-mode metadata. Continue executing through existing bash code unless explicitly enabled.
- `src/test_runner/`: add helper conversion from `TestIntent` to `TestRunRequest` or validated custom command. Keep current `/test` and model-facing test tool paths intact.
- `src/shell/projector.rs`: add route enum compatibility but do not remove existing selector.
- `src/permission/`: do not alter core permission behavior yet. Add conversion helpers only if low risk.

## Tests

Add planner fixture tests:

- `cargo test` plans to `TestRunner`.
- `cargo nextest run` plans to `TestRunner` or validated custom command if supported.
- `pytest` and `uv run pytest` plan to `TestRunner`.
- `git status` plans to native git read route.
- `git diff` plans to git diff projector and RTK eligibility above threshold.
- `git commit -m x` plans to git write with ask permission.
- `rg foo src` plans to file search projector.
- `python script.py` plans to Python stub with script intent.
- `python -c ...` plans to Python stub but high risk.
- `cargo test && rm -rf .` remains complex shell or reject; it must not plan to test runner.

Add property-style regression tests for shell smuggling if feasible.

## Acceptance criteria

- `CommandPlan` and backend route types compile and are covered by tests.
- Planner is deterministic and does not require model calls.
- Known simple commands produce expected backend/projector/permission metadata.
- Complex shell is never routed into native/test/Python backends as if safe.
- Existing bash and test runner behavior remains unchanged by default.
- RTK policy metadata exists and does not require an RTK binary.

## Suggested validation commands

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib test_runner::custom
cargo test -p codegg --lib test_runner
cargo test --test shell_projection_harness
```

Broader fallback:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Risks and mitigations

The main risk is premature routing. Mitigate by making the planner observable but not active by default. Another risk is duplicate permission logic. Mitigate by treating command permissions as planning metadata until they are deliberately bridged into the existing permission registry in a later phase.
