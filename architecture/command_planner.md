# Command Planner

The command planner maps classified intents to execution backends, generates permission requests, and selects projection/RTK policies. This is the second stage of the command intent pipeline (classify → **plan** → route).

## Source

`src/command_intent/plan.rs` (re-exported via `src/command_planner.rs`)

## Core Types

### `ExecutionBackend`

```rust
pub enum ExecutionBackend {
    RawShell { command: String },
    ManagedArgv { argv: Vec<String>, cwd: PathBuf },
    NativeTool { tool_name: String },
    TestRunner { validated_command: Option<String> },
    PythonScript { script: String, mode_guess: PythonModeGuess },
    Reject { reason: String },
}
```

Methods: `label()` → `&str`, `is_executable()` → `bool`.

### `PythonModeGuess`

`Analyze`, `Transform`, `Verify`, `Unknown` — guessed from intent kind.

### `ProjectorRoute` (9 variants)

| Variant | Used for |
|---------|----------|
| `Raw` | Mutations, file writes/edits |
| `Truncated` | Raw shell fallback |
| `ErrorRetention` | Build/lint/format |
| `GitStatus` | `git status` |
| `GitDiff` | `git diff` |
| `GitLog` | `git log` |
| `TestReport` | Test commands |
| `FileSearch` | Search/list/read |
| `PythonRun` | Python scripts |
| `RtkEligible(Box<ProjectorRoute>)` | Wraps another route when RTK compression is eligible |

### `PlanRtkPolicy`

```rust
pub enum PlanRtkPolicy {
    Disabled,
    Eligible { min_raw_bytes: usize, preserve_exact_spans: Vec<ProjectionSpanKind>, goal: CompressionGoal },
    RequiredForPromotion,
}
```

### `CommandPlan`

```rust
pub struct CommandPlan {
    pub intent: CommandIntent,
    pub backend: ExecutionBackend,
    pub permission_requests: Vec<CommandPermissionRequest>,
    pub projector: ProjectorRoute,
    pub rtk_policy: PlanRtkPolicy,
    pub context_policy: ContextPolicy,
    pub timeout_secs: Option<u64>,
    pub cwd: Option<PathBuf>,
    pub notes: Vec<String>,
}
```

Methods: `is_executable()`, `requires_any_permission()`.

## Planning

```rust
pub fn plan_execution(intent: &CommandIntent) -> CommandPlan
```

### Backend Selection

| IntentKind | Backend |
|------------|---------|
| Test | `TestRunner { validated_command }` |
| PythonAnalyze/Transform/Verify | `PythonScript { script, mode_guess }` |
| GitReadOnly | `NativeTool { tool_name: "egggit" }` |
| GitMutating | `RawShell { command }` |
| SearchReadOnly, FileRead, Build, Lint, Format | `ManagedArgv { argv, cwd }` |
| FileWrite, FileEdit, RawShell | `RawShell { command }` |
| Rejected | `Reject { reason }` |

### Projector Selection

| IntentKind | ProjectorRoute |
|------------|----------------|
| GitReadOnly | `GitDiff` / `GitLog` / `GitStatus` (by command prefix) |
| GitMutating | `Raw` |
| Test | `TestReport` |
| SearchReadOnly, FileRead | `FileSearch` |
| Python* | `PythonRun` |
| Build/Lint/Format | `ErrorRetention` |
| FileWrite, FileEdit | `Raw` |
| RawShell | `Truncated` |

### RTK Policy

- Test: Eligible (4096 min, preserve failure names/paths/line numbers)
- git diff: Eligible (2048 min, preserve diff hunks/file paths/line numbers)
- Python: Eligible (2048 min, preserve compiler errors/file paths/line numbers)
- RawShell: Eligible (4096 min, reduce tokens)
- SearchReadOnly: Eligible (4096 min, reduce tokens)
- All others: Disabled

### Timeouts

| Family | Timeout |
|--------|---------|
| Test | 300s |
| Build | 120s |
| PythonAnalyze/Transform | 60s |
| PythonVerify | 300s |
| GitReadOnly | 30s |
| GitMutating | 60s |
| SearchReadOnly | 30s |
| Others | None |

### Permission Generation

`generate_permission_requests()` maps each `ExecutionCapability` in the intent's risk assessment to a `CommandPermissionRequest` with a default decision (`Allow`/`Ask`/`Deny`). Reject backends produce no permissions.

## Re-exports

`src/command_planner.rs` re-exports everything from `command_intent::plan`:
```rust
pub use crate::command_intent::plan::{
    plan_execution, CommandPermissionRequest, CommandPlan, CompressionGoal, ExecutionBackend,
    PermissionDefault, PlanRtkPolicy, ProjectionSpanKind, ProjectorRoute, PythonModeGuess,
};
```

## Tests

```bash
cargo test -p codegg --lib command_intent
```
