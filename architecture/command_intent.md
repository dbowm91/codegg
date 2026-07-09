# Command Intent

Command intent classification analyzes shell commands to determine their family, risk level, execution capabilities, and context policy. This is the first stage of the command intent pipeline (classify → plan → route).

## Source

`src/command_intent/mod.rs`

## Core Types

### `CommandIntentKind` (16 variants)

| Variant | Description |
|---------|-------------|
| `Test` | Test execution commands (cargo test, pytest, go test, npm test, etc.) |
| `GitReadOnly` | Read-only git (status, diff, log, show, branch, remote, stash list, tag) |
| `GitMutating` | Git mutations (commit, push, reset, clean, etc.) |
| `SearchReadOnly` | Search/list/read (rg, grep, find, ls, tree, cat, head, tail, wc) |
| `FileRead` | File reading (cat, less, more, head, tail, type) |
| `FileWrite` | File writing |
| `FileEdit` | File editing |
| `Build` | Build commands (cargo build/check/clippy/fmt/run, make, cmake, npm run) |
| `Lint` | Linting |
| `Format` | Formatting |
| `PythonAnalyze` | Python read-only analysis |
| `PythonTransform` | Python mutating transformation |
| `PythonVerify` | Python test/verification |
| `RawShell` | Unrecognized or complex shell commands |
| `Rejected` | Empty or invalid commands |

### Supporting Enums

```rust
pub enum CommandSource { AgentTool, HumanShell, TestRunner, PythonScript, Unknown }
pub enum CommandOrigin { BashTool, TestSlashCommand, HumanShellBang, HumanShellDoubleBang, PythonScripting, DirectExecution }
pub enum IntentConfidence { High, Medium, Low, Unknown }
pub enum RiskLevel { Safe, Low, Medium, High, Critical }
pub enum ContextPolicy { ProjectToModel, LocalOnly, StoreOnly, Promote }
```

### `ExecutionCapability` (10 variants)

`ReadWorkspace`, `WriteWorkspace`, `Subprocess`, `Network`, `EnvAccess`, `DependencyInstall`, `OutsideWorkspace`, `DestructiveFileMutation`, `GitMutation`, `ContextPromotion`

### `RiskAssessment`

```rust
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub reasons: Vec<String>,
    pub capabilities: Vec<ExecutionCapability>,
}
```

Constructors: `safe()`, `low(reason)`, `medium(reason)`, `high(reason)`.

### `CommandIntent`

```rust
pub struct CommandIntent {
    pub kind: CommandIntentKind,
    pub confidence: IntentConfidence,
    pub risk: RiskAssessment,
    pub source: CommandSource,
    pub command: String,
    pub context_policy: ContextPolicy,
}
```

Methods:
- `is_safe_for_model_context()` — true when risk is Safe/Low AND context is ProjectToModel/Promote
- `requires_permission()` — true when risk is Medium/High/Critical

## Classification

```rust
pub fn classify_command(command: &str) -> CommandIntent
```

Classification order:
1. Empty → `Rejected`
2. Shell operators (`|`, `;`, `$`, `` ` ``, `&`) detected by `has_shell_operators()` → `RawShell` (Low confidence, medium risk)
3. Test patterns → `Test`
4. Python patterns → `PythonAnalyze|Transform|Verify`
5. Git commands → `GitReadOnly` or `GitMutating`
6. File read patterns → `FileRead`
7. Search patterns → `SearchReadOnly`
8. Build patterns → `Build`
9. Unmatched → `RawShell` (Low confidence)

### Shell Operator Detection

`has_shell_operators()` uses quote-aware scanning to detect `|`, `;`, `$`, `` ` ``, `&` outside quotes. Commands with operators are classified as `RawShell` — this prevents `cargo test && rm -rf .` from routing to the test runner.

### Pattern Recognition

Classification uses `looks_like_*` / `classify_*` helper pairs. Key patterns:

- **Test**: `cargo test`, `cargo nextest`, `pytest`, `uv run pytest`, `go test`, `npm/pnpm/yarn/bun test`, `make test/check`
- **Python**: `python`, `python3`, `uv run python/pytest`, `pytest`, `.py` suffix
- **Git readonly**: `status`, `diff`, `log`, `show`, `branch`, `remote`, `stash list`, `tag`
- **Search**: `rg`, `grep`, `find`, `ls`, `tree`, `cat`, `head`, `tail`, `wc`, `which`, `whereis`
- **Build**: `cargo build/check/clippy/fmt/run`, `make`, `cmake`, `npm/pnpm run`

## Integration

`classify_command()` is called by:
- `BashTool::execute()` in `src/tool/bash.rs` — attaches routing metadata when `CommandIntentConfig` is set
- `plan_execution()` in `src/command_intent/plan.rs` — second stage of the pipeline

## Tests

```bash
cargo test -p codegg --lib command_intent
```
