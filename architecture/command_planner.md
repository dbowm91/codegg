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
    GitMutating { tool_name: String, argv: Vec<String> },
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

| IntentKind | Backend | argv source |
|------------|---------|-------------|
| Test | `TestRunner { validated_command }` | command string validated |
| PythonAnalyze/Transform/Verify | `PythonScript { script, mode_guess }` | command string |
| GitReadOnly | `NativeTool { tool_name: "egggit" }` | n/a |
| GitMutating (safe) | `GitMutating { tool_name, argv }` | `parsed_argv` |
| GitMutating (dangerous) | `RawShell { command }` | command string |
| SearchReadOnly, FileRead | `ManagedArgv { argv, cwd }` | `parsed_argv` (fallback to whitespace split) |
| Build, Lint, Format | `ManagedArgv { argv, cwd }` | `parsed_argv` (fallback to whitespace split) |
| FileWrite, FileEdit, RawShell | `RawShell { command }` | command string |
| Rejected | `Reject { reason }` | n/a |

Safe git mutations (→ `GitMutating`): add, commit, stash, checkout, switch, restore, merge, rebase, cherry-pick, revert.

Dangerous git mutations (→ `RawShell`): push, pull, reset --hard, clean -f, branch -D.

`ManagedArgv` backends use `intent.parsed_argv` from the shell shape parser, falling back to whitespace splitting if `None`.

### `validate_for_active_routing()`

Validates a `CommandPlan` before active routing dispatch. All 7 checks must pass:

1. **SimpleArgv shape** — `intent.parsed_argv` must be `Some` (no complex shell)
2. **High confidence** — `intent.confidence` must be `High`
3. **Non-RawShell/Reject backend** — backend must be a structured type
4. **Non-Critical risk** — `intent.risk.level` must not be `Critical`
5. **No DestructiveFileMutation** — risk capabilities must not include `DestructiveFileMutation`
6. **No OutsideWorkspace** — risk capabilities must not include `OutsideWorkspace`
7. **No pending permissions** — all permission requests must be pre-resolved (no `Ask` defaults in active mode)

Returns `Ok(CommandPlan)` if all checks pass, `Err(reason)` otherwise. Failed validation falls back to raw shell execution.

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

`generate_permission_requests()` maps each `ExecutionCapability` in the intent's risk assessment to a `CommandPermissionRequest` with a context-aware default decision (`Allow`/`Ask`/`Deny`). Reject backends produce no permissions.

| Capability | Default | Rationale |
|------------|---------|-----------|
| `ReadWorkspace` | `Allow` | Read-only, safe |
| `Subprocess` | `Allow` | Command execution is expected |
| `EnvAccess` | `Allow` | Environment reads are expected |
| `ContextPromotion` | `Allow` | Output promotion is a user action |
| `Network` | `Ask` | Network access needs user consent |
| `WriteWorkspace` | `Ask` for writing formatters, `Allow` for read-only formatters, `Ask` otherwise | `cargo fmt --check`, `prettier --check` auto-allowed; writing formatters ask; other writes ask |
| `GitMutation` | `Allow` for `git add` only, `Ask` otherwise | Only `git add` is safe to auto-allow; commit/checkout/switch/restore/stash push all ask (may run hooks or overwrite worktree) |
| `DependencyInstall` | `Deny` | Package installs mutate global state |
| `OutsideWorkspace` | `Deny` | Access outside workspace is unsafe |
| `DestructiveFileMutation` | `Deny` | Destructive operations are blocked |

The `is_formatter_command()` helper checks the intent kind (Format) and command text for `cargo fmt`, `prettier`, `black`, `isort`, `rustfmt`. The `is_read_only_formatter()` helper detects `--check`, `--diff`, and `checkfmt` in the command string. The `is_safe_git_subcommand()` helper checks parsed argv for the single safe subcommand: `git add`.

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

Includes 21 new routing/validation tests for `validate_for_active_routing()` and `GitMutating` backend selection.
