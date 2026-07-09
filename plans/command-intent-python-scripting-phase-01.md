# Phase 01: Command Intent Core Model

## Objective

Introduce a conservative command intent model that can describe executable actions before they are planned, permissioned, executed, projected, or compressed. This phase should not materially alter runtime behavior. It should establish types, classifiers, fixtures, and documentation so later phases can route natural shell/Python idioms into codegg subsystems safely.

## Motivation

codegg already has shell projection, bounded shell output storage, RTK projection hooks, and a supervised test runner. The missing abstraction is a shared semantic representation of agent/human executable intent. Without it, bash, tests, git commands, searches, and future Python scripts remain separate pathways with overlapping policy and projection logic.

The first phase should make command intent explicit while preserving existing behavior.

## Scope

Implement the core model and a conservative classifier for simple command forms. Add tests and fixtures. Wire classification into shell/test code only as non-disruptive metadata/logging or behind an internal test helper. Do not reroute execution yet.

## Proposed module layout

Prefer a new module under `src/command_intent/` or `src/execution/intent/` rather than placing the model directly inside `src/shell/`. The intent model should be usable by shell, test runner, Python scripting, git routing, and tools.

Suggested files:

```text
src/command_intent/
  mod.rs
  types.rs
  classify.rs
  shell_syntax.rs
  risk.rs
  fixtures.rs        # optional test-only helper, behind cfg(test)
```

If maintainers prefer to keep this close to existing shell machinery initially, use `src/shell/intent.rs`, but design the public types so they can move to a shared execution module later.

## Core types

Add the following conceptual types. Names can be adjusted to match repo style.

```rust
pub struct CommandIntent {
    pub id: Option<CommandRunId>,
    pub origin: CommandOrigin,
    pub source: CommandSource,
    pub cwd: PathBuf,
    pub kind: CommandIntentKind,
    pub confidence: IntentConfidence,
    pub risk: RiskAssessment,
    pub capture_policy: CapturePolicy,
    pub context_policy: ContextPolicy,
}

pub enum CommandSource {
    ShellCommand(String),
    Argv(Vec<String>),
    PythonScript { code: String, entrypoint: PythonEntrypoint },
    NativeToolCall { tool_name: String, input: serde_json::Value },
}

pub enum CommandOrigin {
    HumanEphemeral,
    HumanPromoted,
    AgentTool,
    SlashCommand,
    SystemInternal,
}

pub enum CommandIntentKind {
    RawShell,
    ComplexShell,
    TestRun(TestIntent),
    GitRead(GitReadIntent),
    GitWrite(GitWriteIntent),
    FileRead(FileReadIntent),
    FileSearch(SearchIntent),
    FileMutation(FileMutationIntent),
    PythonScript(PythonScriptIntent),
    RtkOperation(RtkIntent),
    PackageManager(PackageManagerIntent),
    Unknown,
}

pub enum IntentConfidence {
    Exact,
    High,
    Medium,
    Low,
    Fallback,
}
```

For Phase 01, subordinate intent structs may be intentionally small. Do not overfit. For example:

```rust
pub struct TestIntent {
    pub runner: TestRunnerKind,
    pub argv: Vec<String>,
}

pub enum TestRunnerKind {
    CargoTest,
    CargoNextest,
    Pytest,
    UvRunPytest,
    NpmTest,
    PnpmTest,
    YarnTest,
    BunTest,
    GoTest,
    ZigBuildTest,
    MakeTest,
    MakeCheck,
    Unknown,
}
```

## Shell syntax classification

Do not build a full shell parser. Add a minimal helper that classifies shell command text into one of:

```rust
pub enum ShellShape {
    Empty,
    SimpleArgv(Vec<String>),
    ComplexShell { reasons: Vec<ShellComplexityReason> },
}
```

A command should become `ComplexShell` if it contains shell syntax that cannot be safely interpreted as a simple argv vector:

- control operators: `;`, `&&`, `||`, `&`, `|`;
- redirection: `>`, `>>`, `<`, `2>`, `2>&1`;
- command substitution: `$(`, `${`, backticks;
- heredocs;
- newlines;
- shell grouping: `(`, `)`, `{`, `}`;
- globs or expansion if classification depends on literal files;
- env assignment prefixes when unsupported;
- unbalanced quotes or parser failure.

Use an existing shell-words style parser if already available. If not, a simple conservative parser is acceptable. False negatives that force raw shell are preferable to false positives that misroute commands.

## Initial classifier coverage

Implement exact/high-confidence classification for simple argv forms only.

Read-only git:

- `git status`
- `git status --short`
- `git diff`
- `git diff --staged`
- `git log`
- `git log --oneline`
- `git show`
- `git branch --show-current`

Tests:

- `cargo test ...`
- `cargo nextest ...`
- `pytest ...`
- `python -m pytest ...`
- `uv run pytest ...`
- `npm test ...`
- `pnpm test ...`
- `yarn test ...`
- `bun test ...`
- `go test ...`
- `zig build test ...`
- `make test`
- `make check`

Search/list/read:

- `rg ...`
- `fd ...`
- `find ...` only for simple read/list forms, not `-exec`;
- `ls ...`
- `pwd`
- `cat <file>` only for simple single-file reads;
- `sed -n ... <file>` only if safely parseable.

Python:

- `python script.py ...`
- `python3 script.py ...`
- `python -m module ...` should classify as Python only if module is not `pytest`; `python -m pytest` should classify as `TestRun`.
- `python -c ...` and heredoc forms should be identified as Python intent but marked high risk or complex source; actual routing is later.

Mutating git/filesystem:

- `git add`, `git commit`, `git restore`, `git checkout`, `git reset`, `git clean`, `git push` should classify as `GitWrite` with elevated risk, but Phase 01 should not reroute.
- `rm`, `mv`, `cp`, `chmod`, `chown`, redirections, and write-like Python scripts should classify as file mutation or complex shell with elevated risk.

Unknown:

- Anything not recognized should remain `RawShell` or `Unknown` depending on shell shape.

## Risk model

Add a lightweight `RiskAssessment` in Phase 01.

```rust
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub capabilities: BTreeSet<ExecutionCapability>,
    pub reasons: Vec<String>,
}

pub enum RiskLevel {
    Low,
    Medium,
    High,
    Destructive,
    Unknown,
}

pub enum ExecutionCapability {
    ReadWorkspace,
    WriteWorkspace,
    ReadOutsideWorkspace,
    WriteOutsideWorkspace,
    SpawnSubprocess,
    Network,
    AccessEnvironment,
    InstallDependency,
    GitRead,
    GitWrite,
    DeleteFiles,
    ShellEval,
}
```

Keep this intentionally conservative. Risk is used for planner/policy phases later.

## Integration points for Phase 01

Add classification helpers that can be called from:

- `BashTool::execute` before existing security checks, for debug tracing only;
- human shell submission path, for optional shell entry metadata only;
- test runner custom command validator tests, to ensure shared command semantics do not conflict.

Do not modify the existing bash security behavior yet. In particular, do not unblock `python -c` in `BashTool` during this phase.

## Documentation updates

Create or update architecture docs:

- Add `architecture/command_intent.md` describing the intent model, shell shape classification, conservative parser policy, non-goals, and future planner/projection phases.
- Reference this roadmap from existing shell/test architecture docs only if the repo convention supports cross-links.

## Tests

Add fixture-driven tests for classifier behavior.

Suggested tests:

```text
cargo test -p codegg --lib command_intent
```

Fixture categories:

- simple recognized test commands;
- simple read-only git commands;
- high-risk git write commands;
- simple search/list commands;
- Python script/module/one-liner forms;
- complex shell fallback cases;
- shell smuggling attempts such as `cargo test && rm -rf .`, `git status; cat /etc/passwd`, `pytest | sh`, `python -c '...'`, and heredocs;
- Unicode/bidi/control-character rejection or complexity classification.

Assertions should verify intent kind, confidence, risk level, capability set, and complex-shell reason list.

## Acceptance criteria

- New command intent types compile and are covered by unit tests.
- Classifier recognizes the initial command families conservatively.
- Complex shell is detected and never misclassified as a safe simple command.
- Existing bash, shell, and test runner behavior remains unchanged.
- Existing documented test commands still pass for affected modules.
- Architecture documentation clearly states that this phase is observe/classify only.

## Suggested validation commands

Run the narrowest relevant commands first:

```bash
cargo test -p codegg --lib command_intent
cargo test -p codegg --lib test_runner::custom
cargo test -p codegg --lib shell::rtk
cargo test --test shell_projection_harness
```

If broader validation is needed:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Risks and mitigations

The main risk is overclassification. Mitigate by defaulting to `ComplexShell` or `RawShell` whenever syntax is ambiguous. Another risk is creating types that duplicate existing shell/test types. Mitigate by keeping Phase 01 types shallow and avoiding execution behavior changes until the planner phase.
