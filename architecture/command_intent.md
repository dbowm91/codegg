# Command Intent

Command intent classification analyzes shell commands to determine their family, risk level, execution capabilities, and context policy. This is the first stage of the command intent pipeline (classify → plan → route).

## Source

`src/command_intent/mod.rs` (with `shell_shape.rs` and `plan.rs` submodules)

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

Constructors: `safe()`, `low(reason)`, `medium(reason)`, `high(reason)` (backward-compat), plus specific constructors: `read_only(reason)` (no Subprocess), `raw_shell(reason)` (with Subprocess), `managed_process(reason)` (no Subprocess), `git_mutation(reason)` (with GitMutation), `destructive(reason)` (with DestructiveFileMutation).

### `CommandIntent`

```rust
pub struct CommandIntent {
    pub kind: CommandIntentKind,
    pub confidence: IntentConfidence,
    pub risk: RiskAssessment,
    pub source: CommandSource,
    pub command: String,
    pub context_policy: ContextPolicy,
    pub parsed_argv: Option<Vec<String>>,
}
```

`parsed_argv` is populated for all simple argv-shaped commands. `None` for complex shell commands where argv parsing failed or was not applicable.

Methods:
- `is_safe_for_model_context()` — true when risk is Safe/Low AND context is ProjectToModel/Promote
- `requires_permission()` — true when risk is Medium/High/Critical

Permission defaults for each `ExecutionCapability` are defined in the command planner's `generate_permission_requests()` — see [command_planner.md](command_planner.md#permission-generation).

## Classification

```rust
pub fn classify_command(command: &str) -> CommandIntent
pub fn classify_command_with_context(command: &str, ctx: &CommandIntentContext) -> CommandIntent
```

`classify_command()` is a backward-compatible wrapper that calls `classify_command_with_context` with a default context (uses process cwd). For workspace-aware path checks, use `classify_command_with_context` with a `CommandIntentContext` containing the workspace root.

```rust
pub struct CommandIntentContext {
    pub workspace_root: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
}
```

Classification is now driven by **shell shape parsing** (`src/command_intent/shell_shape.rs`):

1. Parse command into `ShellShape` via `parse_shell_words()`
2. `Empty` → `Rejected`
3. `ComplexShell { reasons }` → `RawShell` (Low confidence, medium risk)
4. `SimpleArgv(argv)` → pattern-match on first token through `looks_like_*` / `classify_*` helpers

### ShellShape Parsing

`parse_shell_words()` is a POSIX-aware state machine that handles:
- Single quotes (no escape processing inside)
- Double quotes (with `\"` and `\\` escapes)
- Backslash escapes outside quotes
- Detection of shell complexity: pipes, semicolons, `&&`/`||`, background, redirection, command substitution, variable expansion, heredocs, globs, tilde, env assignments, unbalanced quotes

Commands classified as `ComplexShell` are routed to `RawShell` — this prevents `cargo test && rm -rf .` from routing to the test runner.

### Pattern Recognition

Classification uses `looks_like_*` / `classify_*` helper pairs. Key patterns:

- **Test**: `cargo test`, `cargo nextest`, `pytest`, `uv run pytest`, `go test`, `npm/pnpm/yarn/bun test`, `make test/check`
- **Python**: `python`, `python3`, `uv run python/pytest`, `pytest`, `.py` suffix
- **Git readonly**: `status`, `diff`, `log`, `show` (always read-only); `branch`, `tag`, `remote`, `stash` (read-only only for specific forms — see git classification below)
- **Search**: `rg`, `grep`, `find`, `ls`, `tree`, `wc` (with destructive-flag and outside-workspace rejection)
- **File read**: `cat`, `less`, `more`, `head`, `tail` (with outside-workspace rejection)
- **Build**: `cargo build/check/clippy/fmt/run`, `make`, `cmake`, `npm/pnpm run`

### Git Classification

`classify_git()` delegates to **codegg-git's typed parser** (`crates/codegg-git`) for accurate risk assessment. The parser parses git argv into a structured `GitOperation` with typed subcommands, flags, and argument positions, enabling precise read-only vs. mutating classification without string-prefix heuristics.

When the typed parser fails (unknown subcommand, malformed argv, or parse error), classification **falls back to lightweight heuristics** — first-token matching on known subcommands with conservative risk assignment (falls through to `RawShell` for unrecognized forms).

Known mutating operations identified by the typed parser include: `add`, `commit`, `stash` (non-list forms), `branch` (create/delete/rename), `tag` (create/delete), `remote` (add/remove/rename/set-url), `push`, `pull`, `reset`, `clean`, `checkout` (branch switching), `switch`, `restore`, `merge`, `rebase`, `cherry-pick`, `revert`. Risk levels are derived from the operation type and flags (e.g., `--hard` on `reset`, `-f` on `clean`).

**Polish-pass provenance parity:** The execution-origin matrix in `tests/git_execution_origin_matrix.rs` (19 tests, rows 1-10) asserts that every origin — native typed read/mutation, native raw git subcommand, Bash simple git read/mutation, managed git argv fallback, raw shell with `|`/`&&`/`;`, TUI git action, daemon git action, replay/rerun — has consistent planned backend, env policy, redaction boundary, and RunStore ownership. Row 5 documents the Bash simple git mutation gap: it classifies as `GitMutating` but dispatches as `RawShell` (see [command_planner.md routing caveat](command_planner.md#planning)).

### Search/read Classification

- `find -exec`, `-delete`, `-ok`, `-execdir` → rejected from safe search (falls through to RawShell)
- Absolute outside-workspace path arguments → rejected from safe search and file-read
- `which`/`whereis` → NOT classified as file reads (fall through to RawShell)

## Integration

`classify_command()` is called by:
- `BashTool::execute()` in `src/tool/bash.rs` — attaches routing metadata when `CommandIntentConfig` is set

### CommandIntentMode

`CommandIntentMode` enum (`Observe` | `Active` | deprecated `Route`, default
`Observe`) controls whether the bash tool only observes intent or attempts
active routing. `Observe` classifies and annotates metadata; `Active` dispatches
to structured backends when the command validates for routing. `Route` remains a
backward-compatible alias for `Active`; new configuration should use `Active`.

### CommandIntentFamily

`CommandIntentFamily` enum with 7 variants, used for per-family active routing config:

```rust
pub enum CommandIntentFamily {
    Tests,
    GitRead,
    Search,
    Python,
    Build,
    Lint,
    Format,
}
```

### RouteLevel (per-family config)

```rust
pub enum RouteLevel {
    Off,      // no routing for this family
    Observe,  // classify + annotate only (default)
    Active,   // dispatch to structured backend
}
```

Per-family fields in `CommandIntentConfig`:
- `route_build: Option<RouteLevel>` — Build family (cargo build/check)
- `route_lint: Option<RouteLevel>` — Lint family (cargo clippy, mypy, pyright, tsc)
- `route_format: Option<RouteLevel>` — Format family (cargo fmt, prettier, black)

`is_active_for_family(family)` returns true when the family's `RouteLevel` is `Active`. `family_level(family)` returns the effective `RouteLevel` for a family (family-specific overrides fall back to global mode).

### Build/Lint/Format Classification

Expanded classification for build-adjacent families:

- **Build**: `cargo build`, `cargo check`, `make`, `cmake`, `npm run build`
- **Lint**: `cargo clippy`, `mypy`, `pyright`, `tsc`
- **Format**: `cargo fmt`, `prettier`, `black`
- **Typecheck** is folded into Lint (mypy, pyright, tsc are all static analysis)

Package managers (`npm install`, `pip install`, `cargo install`, etc.) are **NOT** classified as Build — they fall through to `RawShell`. This is a safety boundary: package installs mutate global state and should not be auto-routed.

## Tests

```bash
cargo test -p codegg --lib command_intent
```

Tests cover: general classification, shell shape parsing (quoted args, operators, complex shell detection), git read-only/mutating classification (branch/tag/remote forms), search/read rejection (find -exec, outside-workspace, which/whereis), build/lint/format/typecheck family classification, package manager safety boundary, parsed argv round-trips, and cross-module classify→plan→route integration.
