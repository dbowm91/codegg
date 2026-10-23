# Command Intent

Command intent classification analyzes shell commands to determine their family, risk level, execution capabilities, and context policy. This is the first stage of the command intent pipeline (classify → plan → dispatch). The third stage, dispatch, is owned by `CommandPlan::dispatch_target()`; `architecture/command_routing.md` is a compatibility facade over that same mapping, not a third stage with its own logic.

## Purpose

Classify shell commands into typed intent families, assess risk, and produce a `CommandIntent` that downstream planner and routing stages consume.

## Where It Lives

- `src/command_intent/mod.rs` — classification logic and types
- `src/command_intent/shell_shape.rs` — POSIX-aware shell word parser
- `src/command_intent/plan.rs` — execution planning (re-exported via `src/command_planner.rs`)

## How It Works

```
  raw command string
        │
        ▼
  parse_shell_words() ─── Empty ──→ Rejected
        │
        ▼
  SimpleArgv(argv)        ComplexShell { reasons } ──→ RawShell (Low)
        │
        ▼
  looks_like_test?  ─yes→ classify_test()
  looks_like_python? yes→ classify_python()
  looks_like_git?   ─yes→ classify_git()       ← codegg-git typed parser
  looks_like_file_read? → classify_file_read()
  looks_like_search? ─yes→ classify_search()
  looks_like_build?  ─yes→ classify_build()
        │
        ▼ (no match)
  RawShell (Low confidence, unclassified)
```

## Core Types

### `CommandIntentKind` (15 variants)

| Variant | Description |
|---------|-------------|
| `Test` | Test execution (cargo test, pytest, go test, npm test, etc.) |
| `GitReadOnly` | Read-only git (status, diff, log, show, branch, remote, stash list, tag) |
| `GitMutating` | Git mutations (commit, push, reset, clean, etc.) |
| `SearchReadOnly` | Search/list/read (rg, grep, fd, find, ls, pwd, wc) |
| `FileRead` | File reading (cat, less, more, head, tail) |
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
pub enum CommandSource {
    AgentTool, HumanShell, TestRunner, PythonScript, Unknown
}
pub enum CommandOrigin {
    BashTool, TestSlashCommand, HumanShellBang,
    HumanShellDoubleBang, PythonScripting, DirectExecution
}
pub enum IntentConfidence { High, Medium, Low, Unknown }
pub enum RiskLevel { Safe, Low, Medium, High, Critical }
pub enum ContextPolicy { ProjectToModel, LocalOnly, StoreOnly, Promote }
```

### `ExecutionCapability` (10 variants)

`ReadWorkspace`, `WriteWorkspace`, `Subprocess`, `Network`, `EnvAccess`,
`DependencyInstall`, `OutsideWorkspace`, `DestructiveFileMutation`,
`GitMutation`, `ContextPromotion`

### `RiskAssessment`

```rust
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub reasons: Vec<String>,
    pub capabilities: Vec<ExecutionCapability>,
}
```

Constructors: `safe()`, `low(reason)`, `medium(reason)`, `high(reason)`,
plus specific: `read_only(reason)` (no Subprocess), `raw_shell(reason)` (with
Subprocess), `managed_process(reason)` (no Subprocess), `git_mutation(reason)`
(with GitMutation), `destructive(reason)` (with DestructiveFileMutation).

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

`parsed_argv` is populated for all simple argv-shaped commands. `None` for
complex shell commands where argv parsing failed or was not applicable.

Methods:
- `is_safe_for_model_context()` — true when risk is Safe/Low AND context is
  ProjectToModel/Promote
- `requires_permission()` — true when risk is Medium/High/Critical

Permission defaults for each `ExecutionCapability` are defined in the command
planner's `generate_permission_requests()` — see
[command_planner.md](command_planner.md#permission-generation).

## Classification

```rust
pub fn classify_command(command: &str) -> CommandIntent
pub fn classify_command_with_context(
    command: &str, ctx: &CommandIntentContext
) -> CommandIntent
```

`classify_command()` is a backward-compatible wrapper that calls
`classify_command_with_context` with a default context (uses process cwd).
For workspace-aware path checks, use `classify_command_with_context` with a
`CommandIntentContext` containing the workspace root.

```rust
pub struct CommandIntentContext {
    pub workspace_root: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
}
```

### ShellShape Parsing

`parse_shell_words()` (`src/command_intent/shell_shape.rs`) is a
POSIX-aware state machine that handles:
- Single quotes (no escape processing inside)
- Double quotes (with `\"` and `\\` escapes)
- Backslash escapes outside quotes
- Detection of shell complexity: pipes, semicolons, `&&`/`||`, background,
  redirection, command substitution, variable expansion, heredocs, globs,
  tilde, env assignments, unbalanced quotes

Commands classified as `ComplexShell` are routed to `RawShell` — this
prevents `cargo test && rm -rf .` from routing to the test runner.

### Pattern Recognition

Classification uses `looks_like_*` / `classify_*` helper pairs. The order
is: Test → Python → Git → FileRead → Search → Build → RawShell fallback.

Key patterns:
- **Test**: `cargo test`, `cargo nextest`, `pytest`, `uv run pytest`, `go test`,
  `npm/pnpm/yarn/bun test`, `make test/check`
- **Python**: `python`, `python3`, `uv run python/pytest`, `pytest`
- **Git readonly**: `status`, `diff`, `log`, `show` (always read-only);
  `branch`, `tag`, `remote`, `stash` (read-only only for specific forms —
  for `stash` the heuristic accepts `list`/`ls`, but the authoritative typed
  parser reads `stash ls` as a push)
- **Search**: `rg`, `grep`, `fd`, `find`, `ls`, `pwd`, `wc`
  (with destructive-flag and outside-workspace rejection)
- **File read**: `cat`, `less`, `more`, `head`, `tail`
  (with outside-workspace rejection)
- **Build**: `cargo build/check/clippy/fmt/run`, `make`, `cmake`,
  `npm/pnpm run`

### Git Classification

`classify_git()` delegates to **codegg-git's typed parser** (`crates/codegg-git`)
for accurate risk assessment. The parser parses git argv into a structured
`GitOperation` with typed subcommands, flags, and argument positions, enabling
precise read-only vs. mutating classification without string-prefix heuristics.

When the typed parser fails (unknown subcommand, malformed argv, or parse
error), classification **falls back to lightweight heuristics** — first-token
matching on known subcommands with conservative risk assignment (falls through
to `RawShell` for unrecognized forms).

**Important typed parser behavior:** The typed parser is authoritative and may
classify a command differently than the heuristic fallback. Two documented cases:

- `git remote -v` is `GitReadOnly` — the typed parser classifies it as read-only,
  and the heuristic fallback agrees (it treats `remote -v` as a read-only form).
- `git remote show origin` is `GitMutating` — the typed parser does not model
  `remote show`, so it falls back to `ManagedGitArgv` and the command is
  mutating, while the heuristic fallback alone would have called it read-only.

The tests `git_branch_contains_is_mutating`, `git_branch_merged_is_mutating`,
`git_remote_v_is_readonly`, `git_remote_show_is_mutating`, `git_stash_ls_is_mutating`,
and `git_remote_get_url_is_readonly` in `src/command_intent/mod.rs` document these
cases. Similarly, `git branch --contains HEAD` and `git branch --merged main` are
classified as `GitMutating` by the typed parser because `--contains`/`--merged`
are treated as unhandled flags, making the positional arg look like a branch to
create. `git stash ls` is likewise `GitMutating`: the typed parser reads `stash
ls` as a push rather than a list, while the heuristic fallback treats `ls` as a
list form.

Known mutating operations identified by the typed parser include: `add`,
`commit`, `stash` (non-list forms), `branch` (create/delete/rename), `tag`
(create/delete), `remote` (add/remove/rename/set-url), `push`, `pull`,
`reset`, `clean`, `checkout` (branch switching), `switch`, `restore`, `merge`,
`rebase`, `cherry-pick`, `revert`. Risk levels are derived from the operation
type and flags (e.g., `--hard` on `reset`, `-f` on `clean`).

**Polish-pass provenance parity:** The execution-origin matrix in
`tests/command_git/git_execution_origin_matrix.inc` (12 test functions,
`row_1` … `row_10` including the `row_5b`/`row_5c` active-mode rows) asserts
that every origin — native typed read/mutation, native raw git subcommand, Bash
simple git read/mutation, Bash managed-argv and typed-git mutation under
`route_git_local_mutation = Active`, managed git argv fallback, raw shell with
`|`/`&&`/`;`, TUI git action, daemon git action, replay/rerun — has consistent
planned backend, env policy, redaction boundary, and RunStore ownership.

Row 5 (`row_5_bash_simple_git_mutation`) pins the **default** behaviour: with
`route_git_local_mutation` unset (the default is `Off`), a bash-translated
simple git mutation classifies as `GitMutating` but dispatches as `RawShell`.
Rows 5b and 5c pin the opt-in path: with `route_git_local_mutation = Active`
the same command dispatches through `RouteToGit` → `GitMutationExecutor`. See
[command_planner.md routing caveat](command_planner.md#backend-selection).

### Search/read Classification

- `find -exec`, `-delete`, `-ok`, `-execdir` → rejected from safe search
  (falls through to RawShell)
- Absolute outside-workspace path arguments → rejected from safe search
  and file-read
- `which`/`whereis` → NOT classified as file reads (fall through to RawShell)

## Integration

`classify_command()` is called by:
- `BashTool`'s shared `execute_inner()` (`src/tool/bash.rs`) calls
  `prepare_command()` (`src/tool/bash.rs`, implementation in
  `src/command_intent/pipeline.rs`), which classifies via
  `classify_command_with_context` (`pipeline.rs`) and attaches routing
  metadata when `CommandIntentConfig` is set

`src/command_intent/pipeline.rs` is the single entry point that parses the raw
command, classifies it, and attaches the routing metadata; `src/tool/bash.rs`
calls it once per invocation.

### CommandIntentMode

`CommandIntentMode` enum (`Observe` | `Active` | deprecated `Route`, default
`Observe`) controls whether the bash tool only observes intent or attempts
active routing. `Observe` classifies and annotates metadata; `Active` dispatches
to structured backends when the command validates for routing. `Route` remains a
backward-compatible alias for `Active`; new configuration should use `Active`.

### CommandIntentFamily

`CommandIntentFamily` enum with 10 variants, used for per-family active
routing config (defined in `crates/codegg-config/src/schema.rs`):

```rust
pub enum CommandIntentFamily {
    Tests,
    GitRead,
    GitLocalMutation,   // add, commit, branch, checkout, restore, stash,
                        // merge/rebase/cherry-pick/revert (no --force/network)
    GitNetwork,         // fetch, pull, push, remote add/remove/set-url, config
    GitDestructive,     // reset --hard, clean -fdx, push --force
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

Per-family fields in `CommandIntentConfig`
(`crates/codegg-config/src/schema.rs`):
- `route_tests: Option<RouteLevel>` — Test family
- `route_git_read: Option<RouteLevel>` — Git read-only family
- `route_search: Option<RouteLevel>` — Search/list/read family
- `route_python: Option<RouteLevel>` — Python family
- `route_build: Option<RouteLevel>` — Build family (cargo build/check)
- `route_lint: Option<RouteLevel>` — Lint family (cargo clippy, mypy, pyright, tsc)
- `route_format: Option<RouteLevel>` — Format family (cargo fmt, prettier, black)
- `route_git_local_mutation: Option<RouteLevel>` — Git local mutation family.
  Default `None`.
- `route_git_network: Option<RouteLevel>` — Git network family. Default `None`.
- `route_git_destructive: Option<RouteLevel>` — Git destructive family.
  Default `None`.

`is_active_for_family(family)` returns true when global mode is Active AND
the family's `RouteLevel` is `Active`. `family_level(family)` returns the
effective `RouteLevel` for a family (family-specific overrides fall back to
global mode: Active mode → `Active`, Observe mode → `Observe`).

### Build/Lint/Format Classification

Expanded classification for build-adjacent families:

- **Build**: `cargo build`, `cargo check`, `cargo run`, `make`, `cmake`,
  `npm/pnpm run build|compile|check`
- **Lint**: `cargo clippy`, `eslint`, `pylint`, `flake8`, `mypy`, `pyright`,
  `tsc`, `rustfmt`, `make lint|typecheck|type-check`,
  `npm/pnpm run lint|typecheck|type-check`
- **Format**: `cargo fmt`, `prettier`, `black`, `isort`, `make format`,
  `npm/pnpm run format`
- **Typecheck** is folded into Lint (mypy, pyright, tsc are all static analysis)

`looks_like_build()` is a gate, not the classifier: it admits `cargo` with a
second token in `{build, check, clippy, fmt, run}`, `npm`/`pnpm run` with a
third token in `{build, compile, check, lint, format, typecheck, type-check}`,
or a first token in `{make, cmake, rustfmt, prettier, black, isort, eslint,
pylint, flake8, mypy, pyright, tsc}`. `classify_build()` then assigns the
family: `cargo fmt` → Format, `cargo clippy` → Lint, every other `cargo` form
→ Build; `npm/pnpm run lint|typecheck|type-check` → Lint and `npm/pnpm run
format` → Format; `eslint|pylint|flake8|mypy|pyright|tsc|rustfmt` → Lint;
`prettier|black|isort` → Format; `make lint|typecheck|type-check` → Lint and
`make format` → Format; everything else → Build.

Package managers (`npm install`, `pip install`, `cargo install`, etc.) are
**NOT** classified as Build — they fall through to `RawShell`. This is a
safety boundary: package installs mutate global state and should not be
auto-routed.

## Background Bash (M002)

`bash.execution_mode=foreground|background` (default foreground) reuses the
same classify/authorize/plan pipeline. Background submits scheduler-backed
routes with trusted owner lineage (`SubmissionKey bash-bg:<digest>`, session
+ turn + invocation key) and subscribes via M001 before returning the
canonical `JobHandle`; it never calls the blocking wait. `RouteToTestRunner`,
`RouteToManagedProcess` (build/lint/format/managed), `RouteToShell`, and
`RouteToPythonScripting` may background; raw-shell background adopts the same
`JobKind::Shell` translation (identical argv/platform launcher, no detached
PID). `RouteToNativeTool`, `RouteToGit`, and `Rejected` fail closed in
background without spawning — no fallback, no reparse, no sandbox downgrade.
`test` direct and `bash`-translated tests share `JobKind::Test` without double
notification (one scheduler terminal → one M001 notice). Reuse the M001
`job` tool for status/wait/cancel/output.

## Invariants & Gotchas

- The typed parser (`codegg_git::parse_git_argv`) is the authoritative
  classifier for git commands. It may disagree with the heuristic fallback
  (e.g., `git remote show origin` → `GitMutating` via the typed parser,
  read-only via the heuristic fallback alone; `git stash ls` → `GitMutating`
  via the typed parser, read-only via the heuristic fallback alone).
  `git remote -v` is `GitReadOnly` under **both** paths.
- Classification order matters: `pytest` matches Test (checked first) before
  Python.
- `ComplexShell` commands always become `RawShell` — even if the first token
  looks like a known family. This prevents `cargo test && rm -rf .` from
  routing to the test runner.
- `parsed_argv` is `None` for `ComplexShell` and `Rejected` commands, which
  causes `validate_for_active_routing()` to reject them.

## Tests

```bash
cargo test -p codegg --lib command_intent
```

Tests cover: general classification, shell shape parsing (quoted args,
operators, complex shell detection), git read-only/mutating classification
(branch/tag/remote forms), search/read rejection (find `-exec`/`-delete`,
absolute paths outside the workspace) and the `which`/`whereis` fall-through to
`RawShell`, build/lint/format/typecheck family classification, package manager
safety boundary, parsed argv round-trips, and cross-module
classify→plan→dispatch integration.

### Track U family split

The original monolithic `CommandIntentFamily::GitMutating` family was split
into three independent families so each can be gated independently via its
own `RouteLevel`:

- **`GitLocalMutation`** — `add`, `commit`, `branch`, `checkout`, `restore`,
  `stash push/pop`, `merge`/`rebase`/`cherry-pick`/`revert` (no `--force`,
  no network). Gated by `route_git_local_mutation`.
- **`GitNetwork`** — `fetch`, `pull`, `push`, `remote add/remove/set-url`,
  `config`. Gated by `route_git_network`.
- **`GitDestructive`** — `reset --hard`, `clean -fdx`, `push --force`.
  Gated by `route_git_destructive`.

The `git_operation_family()` resolver in `src/command_intent/plan.rs:288`
maps `GitOperation` variants to the appropriate family via risk precedence:
`Destructive > Network > Read > LocalMutation`. This replaces the former
`intent_kind_to_family()` mapping that returned `None` for `GitMutating`,
which caused bash-translated simple git mutations to fall back to raw shell
even when active routing was enabled.

## Source Verification

Verified 2026-10-06 against source. Corrected: `CommandIntentFamily`
`schema.rs:2904` → `:3417`; `CommandIntentConfig` `schema.rs:2741` → `:3254`;
`git_operation_family()` `plan.rs:286` → `:288`; execution-origin matrix 19 →
12 tests; the git-parser divergence tests are now named with their real lines
(`src/command_intent/mod.rs:1323/1331/1363/1375` rather than the stale
`:1348-1364`); the integration entry point is now `execute_inner`
(`src/tool/bash.rs:335`) → `prepare_command` (`bash.rs:449`,
`pipeline.rs:31`) rather than the stale `bash.rs:339`. Verified accurate:
`CommandIntentKind` 15 variants, `ExecutionCapability` 10,
`CommandSource` 5, `CommandOrigin` 6, `IntentConfidence` 4, `RiskLevel` 5,
`ContextPolicy` 4, `RiskAssessment` 3 fields, `CommandIntent` 7 fields,
`CommandIntentContext` 2 fields, `CommandIntentFamily` 10 variants,
`CommandIntentMode` (`Observe`/`Active`/deprecated `Route`, default `Observe`),
and the 10 per-family `route_*` config fields.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"), which added 36 lines near the top of
`crates/codegg-config/src/schema.rs`. Corrected: `CommandIntentConfig`
`3254`->`3295` and `CommandIntentFamily` `3417`->`3458`, both re-checked to land
on their declarations. The 10 routing families, `CommandIntentMode` defaults,
and the `Observe`/`Active`/deprecated-`Route` split are unchanged.

Verified 2026-10-23 against source (`src/command_intent/mod.rs`,
`src/command_intent/pipeline.rs`, `src/command_intent/plan.rs`,
`src/command_intent/shell_shape.rs`, `crates/codegg-config/src/schema.rs`,
`src/tool/bash.rs`, `tests/command_git/git_execution_origin_matrix.inc`).
Corrected:

- **Self-contradiction removed.** The "Invariants & Gotchas" bullet claimed
  `git remote -v` is `Mutating` via the typed parser and `ReadOnly` via the
  heuristic — the reverse of the truth, and the reverse of this document's own
  "Git Classification" section. The typed parser and the heuristic both classify
  `git remote -v` as `GitReadOnly` (`git_remote_v_is_readonly`); the genuine
  divergences are `git remote show origin` and `git stash ls`, both
  `GitMutating` via the typed parser and read-only via the heuristic alone.
- **Config line refs replaced by symbol names.** `CommandIntentConfig` and
  `CommandIntentFamily` had drifted past their stated lines in
  `crates/codegg-config/src/schema.rs`; both are now referenced by declaration
  name rather than by line, because the file keeps moving.
- **Integration line refs replaced by symbol names.** `execute_inner()` and the
  `prepare_command()` call site in `src/tool/bash.rs` had drifted; the
  `prepare_command()` / `classify_command_with_context()` pair is now named in
  `src/command_intent/pipeline.rs` without line numbers.
- **Execution-origin matrix described per row.** The matrix has 12 test
  functions, not "rows 1-10"; rows `5b`/`5c` cover the opt-in
  `route_git_local_mutation = Active` path that row 5's default does not.
- **Build/Lint/Format classification filled in.** The previous list named 4
  lint tools and 3 format tools; `classify_build()` in
  `src/command_intent/mod.rs` actually distinguishes `eslint`, `pylint`,
  `flake8`, `mypy`, `pyright`, `tsc`, `rustfmt` (Lint), `prettier`, `black`,
  `isort` (Format), `cargo run`, and the `npm`/`pnpm run` and `make`
  sub-forms. The `looks_like_build()` gate and the `classify_build()` family
  assignment are now both documented.

Verified accurate: `CommandIntentKind` 15 variants, `ExecutionCapability` 10,
`CommandSource` 5, `CommandOrigin` 6, `IntentConfidence` 4, `RiskLevel` 5,
`ContextPolicy` 4, `RiskAssessment` 3 fields, `CommandIntent` 7 fields,
`CommandIntentContext` 2 fields, `CommandIntentFamily` 10 variants,
`CommandIntentMode` (`Observe`/`Active`/deprecated `Route`, default `Observe`),
and the 10 per-family `route_*` config fields; the classification order
(Test → Python → Git → FileRead → Search → Build → RawShell); the
`ComplexShell` → `RawShell` rule; `git_operation_family()` in
`src/command_intent/plan.rs`; the `CommandIntentFamily` / `RouteLevel`
semantics; and the `git_operation_family()` risk precedence
(`Destructive > Network > Read > LocalMutation`).
