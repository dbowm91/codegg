# codegg-git — Typed Git Operation Model

`codegg-git` provides a typed vocabulary for Git commands consumed by the
command-intent classifier, command planner, routing, BashTool dispatch,
the native Git tool, and provenance tracking. It is a pure data-model
and parser library with no dependencies on TUI, provider, Bash, or agent types.

## Module structure

### `operation.rs` — `GitOperation` enum

The central type. 47 variants organized by domain:

| Domain | Variants |
|--------|----------|
| Read-only inspection | `Status`, `Diff`, `DiffStaged`, `Show`, `Log`, `Blame`, `ChangedFiles` |
| Listing | `BranchList`, `RemoteList`, `RemoteGetUrl`, `TagList`, `WorktreeList` |
| Staging | `Add`, `Reset` (with `ResetMode`: `Soft`/`Mixed`/`Hard`/`Merge`/`Keep`) |
| Commit | `Commit` |
| Stash | `StashList`, `StashShow`, `StashPush`, `StashApply`, `StashPop`, `StashDrop` |
| Checkout/Switch/Restore | `Checkout`, `Switch`, `Restore` |
| Branch/Tag create/delete | `BranchCreate`, `BranchDelete`, `BranchRename`, `TagCreate`, `TagDelete`, `TagForceDelete` |
| Merge/Rebase/Cherry-pick/Revert | `Merge`, `Rebase`, `CherryPick`, `Revert` |
| Network | `Fetch`, `Pull`, `Push` |
| Hard reset variants | `ResetHard`, `ResetMixed`, `ResetSoft`, `ResetMerge`, `ResetKeep` |
| Clean | `Clean` |
| Remote | `RemoteAdd`, `RemoteRemove`, `RemoteSetUrl` |
| Config | `ConfigGet`, `ConfigSet`, `ConfigUnset` |
| In-progress control | `Abort`, `Continue`, `Skip` |
| Fallbacks | `ManagedGitArgv { argv, risk }`, `RawShellRequired { argv }` |

Key methods:
- `risk_classes(&self) -> RiskSet` — derives risk from operation variant and flags (e.g. `Push { force: true }` → `NetworkWrite + DestructiveHistory`).
- `subcommand_name(&self) -> &'static str` — returns the git subcommand string for display.

### `risk.rs` — `GitRiskClass` and `RiskSet`

`GitRiskClass` has 11 variants:

| Variant | Meaning |
|---------|---------|
| `ReadOnly` | No side effects |
| `IndexMutation` | Staging index changes (add, reset paths, stash create) |
| `WorktreeMutation` | Working tree file changes (checkout paths, restore, stash apply/pop) |
| `RefMutation` | Branch/tag create, rename, delete |
| `HistoryIntegration` | Commit history rewriting (merge, rebase, cherry-pick, revert, reset --hard) |
| `NetworkRead` | Fetch from remote |
| `NetworkWrite` | Push to remote |
| `RepositoryConfigMutation` | `git config`, remote add/remove |
| `DestructiveWorktree` | Unrecoverable worktree changes (clean -f, checkout --force, reset --hard) |
| `DestructiveHistory` | Unrecoverable history changes (force push, reset --hard, branch -D) |
| `OutsideProject` | References paths outside the project root |

`RiskSet` wraps `Vec<GitRiskClass>` with `is_destructive()` (any `DestructiveWorktree` or `DestructiveHistory`) and `requires_network()` (any `NetworkRead` or `NetworkWrite`).

### `parser.rs` — `parse_git_argv`

```rust
pub fn parse_git_argv(argv: &[String]) -> Result<GitOperation, ParseError>
```

Parses a pre-tokenized `git` argv slice into a `GitOperation`. Input is already split argv — no shell splitting. The parser never executes commands.

Handles 25 subcommands (`status`, `diff`, `show`, `log`, `blame`, `branch`, `tag`, `remote`, `stash`, `checkout`, `switch`, `restore`, `commit`, `add`, `reset`, `clean`, `merge`, `rebase`, `cherry-pick`, `revert`, `fetch`, `pull`, `push`, `config`, `worktree`). Unknown subcommands fall back to `ManagedGitArgv` with conservative risk classification.

### `render.rs` — `render_argv`

```rust
pub fn render_argv(op: &GitOperation) -> Vec<String>
```

Renders a `GitOperation` back into a `git` argv slice. Every variant produces a complete argv beginning with `"git"`. Paths are placed after a literal `"--"` separator when required by git's grammar. No shell quoting is performed — output is raw string tokens suitable for `Command::args()`. Rendering is deterministic.

### `path.rs` — Path safety types

| Type | Purpose |
|------|---------|
| `RepoRoot` | Canonical repository root. Created via `RepoRoot::new(path)` which canonicalizes the path. |
| `RepoPath` | Repository-relative literal path. Rejects NUL bytes, absolute paths, parent traversal (`..`), and paths resolving outside the repository root. Normalizes `./` prefixes. |
| `Pathspec` | Raw advanced pathspec for glob/regex patterns where literal path validation isn't possible. Rejects NUL bytes and empty strings. |

`PathError` has 5 variants: `NullByte`, `AbsolutePath`, `PathEscape`, `Empty`, `NotUtf8`.

### `ref_name.rs` — Ref safety types

| Type | Validation |
|------|-----------|
| `BranchName` | `validate_ref_name`: rejects empty, leading `-`, `..`, `.lock` suffix, `~^:?*[\` chars, NUL bytes |
| `RefName` | Same validation as `BranchName` |
| `RemoteName` | Rejects empty, leading `-`, NUL, `..`, spaces |
| `ObjectId` | 40-char hex (SHA-1) or 64-char hex (SHA-256), rejects non-hex and wrong length |
| `RevisionExpr` | Raw string, only rejects empty (too many forms to validate) |

`RefError` has 7 variants: `Empty`, `IllegalCharacters`, `StartsWithDash`, `DoubleDot`, `LockSuffix`, `SpecialCharacters`, `InvalidObjectId`.

### `error.rs` — `ParseError`

9 variants covering all parser failure modes:

| Variant | Meaning |
|---------|---------|
| `MalformedArgv` | Empty argv or non-git executable |
| `UnsupportedGlobalOption` | Global option the parser can't handle |
| `UnsupportedSubcommand` | Unrecognized git subcommand |
| `AmbiguousSyntax` | Multiple parse interpretations |
| `UnsafePath` | Path failed safety validation |
| `MissingRequiredArgument` | Required flag/argument absent |
| `ContradictoryFlags` | Mutually exclusive flags combined |
| `RequiresManagedFallback` | Operation must use `ManagedGitArgv` fallback |
| `MustRemainRawShell` | Command requires shell semantics |

### `origin.rs` — `GitCommandOrigin`

Metadata enum identifying command provenance: `NativeTool`, `BashTranslation`, `Workflow`, `Tui`. Does not change operation semantics.

## Key invariants

1. **Side-effect free.** Parsing and rendering never execute commands or access the filesystem (beyond `RepoRoot` canonicalization at construction time).
2. **No TUI/provider/Bash/agent dependency.** The crate is a pure data-model and parser library.
3. **Path/ref safety types reject dangerous inputs.** NUL bytes, absolute paths, parent traversal (`..`), and paths resolving outside the repository root are rejected at parse time.
4. **Parser uses pre-tokenized argv.** No whitespace splitting — input is `&[String]` from a prior tokenizer.
5. **Rendering is deterministic.** `render_argv` produces a canonical argv for each variant with no shell quoting.
6. **ManagedGitArgv fallback.** Commands the parser cannot fully represent preserve the original argv with a conservative `RiskSet` derived from heuristic classification.
7. **RawShellRequired.** Commands requiring shell semantics (pipes, redirects, command substitution) are flagged as `RawShellRequired` and cannot be dispatched to structured backends.

## Crate boundary

Phase B consumers (`command_intent`, `command_planner`, `command_routing`, `BashTool`, native Git tool) must consume these types directly. There must be no duplicate parser logic in downstream crates — `parse_git_argv` and `render_argv` are the single source of truth for Git argv parsing and rendering.

## Phase C — Structured reads (egggit)

Phase C extends `egggit` with typed, machine-readable read operations beyond the original `status`/`diff`/`changed_files` surface.

### New modules

| Module | Purpose |
|--------|---------|
| `status_v2` | Rich structured status via `git status --porcelain=v2 -z --branch`. Returns `RichRepoStatus` with branch/detached state, HEAD oid, upstream, ahead/behind, staged/unstaged/untracked/conflict entries, and `DirtySummary`/`OperationState` types. |
| `log` | `log_commits()` → `Vec<CommitInfo>` with oid, parents, author/committer, timestamp, subject, and decorations. |
| `blame` | `blame_file()` → `BlameResult` with per-line `BlameEntry` (oid, author, timestamp, line range). |
| `refs` | `list_branches()`, `list_tags()`, `list_remotes()` → typed `BranchInfo`, `TagInfo`, `RemoteInfo` with upstream/ahead-behind for branches. |

All modules are read-only, async, and delegate to `git` subprocess calls with NUL-delimited or explicit-record-separator output for safe machine parsing.

### GitExecutionService

`src/git_service.rs` provides `GitExecutionService` — a unified executor that:

- Accepts a typed `GitOperation` and repository root;
- Delegates read-only operations to `egggit` for structured parsing;
- Falls back to subprocess execution for mutations and unsupported operations;
- Returns `GitExecutionResult` with `GitPayload` (status, diff, log, branches, tags, remotes, blame, etc.), raw stdout/stderr, exit code, and `ProjectionHints`.

`GitPayload` is the structured payload enum carried on successful read results. Downstream consumers (TUI, tools, projectors) consume `GitPayload` variants instead of parsing raw output.

### Structured git tool actions

`src/tool/git.rs` (`GitTool`) maps subcommands to structured reads via `try_structured_read()`. Read-only subcommands (`status`, `diff`, `log`, `show`, `blame`, `branch`, `tag`, `remote`, `worktree`, `stash`, `rev-parse`, `for-each-ref`) attempt structured execution first; failures fall back to raw subprocess output. Mutations always use raw subprocess execution.

### TUI sidebar structured status

The TUI Git sidebar now consumes `RichRepoStatus` from `status_v2` rather than parsing raw `git status` output. Staged/unstaged/untracked/conflict counts and ahead/behind state are surfaced directly from the structured result. Background execution, timeout, generation-based stale completion rejection, and render purity are preserved.

### Review tool read-only classification

`ReviewTool` uses the unified diff request from `GitExecutionService` and is classified as read-only with model inference. Permission handling reflects repository reads rather than mutations.

## Test coverage

331 tests across `parser`, `operation`, `risk`, `path`, `ref_name`, and `render` modules. Parser tests include property-based testing via `proptest`. Risk classification tests verify each variant produces the expected `RiskSet`. Path/ref tests exercise rejection of all invalid input categories.

Phase C adds dedicated test modules for `status_v2`, `log`, `blame`, and `refs` in `egggit`, plus `git_service` tests in the root crate covering `GitExecutionService` and `GitPayload` construction.
