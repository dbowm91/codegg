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

## Phase D — Local Mutation Executor

Phase D adds typed local mutation execution on top of the read-only infrastructure from Phases A–C. Mutating Git operations no longer shell out to ad-hoc argv; they flow through `src/git_mutations.rs` and `src/git_mutations_ops.rs`, which capture pre/post snapshots, compute a `StateDelta`, pin environment variables, and route runs through `RunStore` for audit and `can_rerun` support.

### Mutation framework (`src/git_mutations.rs`)

| Type | Purpose |
|------|---------|
| `GitEnvPolicy` | Pinned env vars cleared/forced for every `git` invocation (`GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`, `GIT_SEQUENCE_EDITOR=true`, plus `PATH` from host). |
| `RepoSnapshot` | Captured state from `git status --porcelain=v2 -z --branch` — head, branch, detached flag, counts of staged/unstaged/untracked/conflicted. |
| `StateDelta` | Diff between two snapshots plus computed facts (commits created, refs created/deleted, paths staged/unstaged, conflicts). |
| `MutationOutcome` | Completed / NoOp / FastForward / Conflict / Rejected (with reason). |
| `MutationResult` | Full record: operation, subcommand label, delta, outcome, stdout/stderr, exit code, duration. |
| `CommitSelection` | `AlreadyStaged \| StagePaths(Vec<String>) \| StageAll` (defaultable). |
| `GitMutationExecutor` | The runner. Captures before snapshot, renders argv via `codegg-git::render_argv`, applies `GitEnvPolicy`, runs the command, parses stderr for conflict hints, captures after snapshot, computes delta, classifies outcome. |

### Typed helpers (`src/git_mutations_ops.rs`)

Typed wrappers over `GitMutationExecutor`:

* `stage_paths`, `stage_all`, `stage_tracked`
* `unstage_paths`, `unstage_all`
* `commit_with_selection` → `CommitOutcome { mutation, created_oid, amended, empty }`
* `branch_create`, `switch_branch`, `create_and_switch`, `detach_at`, `branch_delete`
* `restore_worktree`, `restore_staged`, `restore_both` (with optional `<source>`)
* `stash_push` (message, include-untracked, paths), `stash_apply`, `stash_pop`, `stash_drop`
* `merge` (revisions, no-ff, allowlisted strategies), `rebase`, `cherry_pick`, `revert`, `abort_in_progress`
* `tag_delete`
* `describe_for_permission` produces a one-line summary suitable for permission prompts.

### Projector (`src/git_mutation_projector.rs`)

`project_mutation(&MutationResult) -> String` formats a structured summary: operation label, before/after snapshot, commits/refs created, paths affected, conflicts, recovery hints, duration.

### Tool integration

* **GitTool** (`src/tool/git.rs`) gains a typed `mutation` action with all variants above (e.g., `"stage_paths"`, `"branch_create"`, `"merge"`, `"revert"`, `"abort"`) plus the existing raw `subcommand` path. Mutations are routed through `GitExecutionService` via `git_mutations_ops` and persisted to `RunStore` with `RunKind::GitMutation`, `PlannedBackend::Git`, `ActualBackend::Git`, `RunOwnership::DelegatedBackend`.
* **CommitTool** (`src/tool/commit.rs`) refactored onto `commit_with_selection` with the new `selection` parameter (`already-staged` default, `stage-paths`, `stage-all`). Stage operations for `stage-paths`/`stage-all` go through `git_mutations_ops::stage_paths` / `stage_all` before LLM-generated messages are produced.
* **ReviewTool** (`src/tool/review.rs`) refactored off `egggit::diff_text` and onto `GitExecutionService::execute` returning a typed `GitPayload::DiffText | DiffSummary | DiffResult`.

### RunStore integration (`src/git_run_store.rs`)

`persist_mutation(store, &MutationResult, workdir, repo_root, backend_family, detail) -> Option<RunId>` writes a `RunDraft { kind: RunKind::GitMutation, ... }` with stdout/stderr artifacts (model-unsafe), state-delta JSON (model-safe), structured summary JSON (model-safe), rerun descriptor carrying `render_argv(&operation)`, and a `RunCompletion` mapping `MutationOutcome::Conflict` and non-zero exits to `RunStatus::Failed` (with conflicts surfaced through `MutationResult.delta.conflicts`). Failures are logged at WARN level and never block the mutation itself.

### Tests

`tests/git_mutations_integration.rs` covers stage/unstage, commit (normal + amend + empty), branch create/switch/delete (with refuse-current), stash push/apply, merge (fast-forward and conflict), rebase, cherry-pick, revert, restore, env-policy (no `GIT_EDITOR` leakage), and projector summary formatting. Tests skip gracefully when `git` is unavailable so CI on minimal containers still passes. 12 tests currently; full suite remains green (7015 tests workspace-wide).

## Phase E — Network, Configuration, and Destructive Operations

Phase E adds typed execution for the three remaining Git families that Phase D deferred: **network operations** (fetch/pull/push/remote), **configuration reads/writes** (`git config`), and **destructive operations** (`git reset`/`git clean`). They share the same executor, projector, and RunStore persistence path as Phase D, and add a new policy layer that restricts credentials, config keys, and destructive scope.

### Network policy (`src/git_network_policy.rs`)

| Type | Purpose |
|------|---------|
| `NETWORK_ALLOWED_ENV_VARS` | Pinned subset (`PATH`, `HOME`, `LANG`, `LC_ALL`, `GIT_TERMINAL_PROMPT`, `GIT_HTTP_LOW_SPEED_LIMIT`, …) used by `NetworkEnvPolicy::apply_to_command` for `git fetch`/`pull`/`push` invocations. |
| `NetworkEnvPolicy` | `apply_to_command(argv, cwd) -> Command` that env-clears the child and restores only the allowed network subset. |
| `NetworkFailureKind` | `Dns \| Connect \| Authentication \| Authorization \| RefRejected \| Timeout \| Transport \| Unknown` — classifier for `stderr` lines emitted by `git fetch`/`push`/etc. |
| `classify_network_failure(stderr, exit_code, timed_out)` | Pattern-matches git stderr (DNS NXDOMAIN, "Authentication failed", "non-fast-forward", "Operation timed out") plus the exit-code/timed-out flags. |
| `redact_url_credentials(url) -> String` | Replaces `user:password@host` with `redacted@host`. Bare `user@host` is preserved (often SSH-key derived). |
| `redact_url_list(urls) -> Vec<String>` | Bulk redaction helper for use in remote listings. |

### Typed network ops (`src/git_network_ops.rs`)

| Helper | `GitOperation` produced | Notes |
|--------|-------------------------|-------|
| `fetch(exec, repo, remote, refspecs, prune, all)` | `Fetch` (typed) or `ManagedGitArgv` (for `--prune`, since the typed parser doesn't model it) | env-pinned, classifies failures. |
| `pull(exec, repo, remote, branch, strategy, ff_only)` | `Pull` with `rebase`/`ff_only` derived from `PullStrategy` enum (`Merge` / `Rebase` / `FastForwardOnly`). | |
| `push(exec, repo, req: PushRequest)` | `Push` with `force`, `force_with_lease`, `set_upstream`, `tags`, `delete` decoded from `PushRequest`. | `PushForce` enum: `Normal \| ForceWithLease { expected_sha } \| Force`. |
| `remote_add`/`remote_remove`/`remote_set_url`/`remote_rename` | `RemoteAdd` / `RemoteRemove` / `RemoteSetUrl` / `ManagedGitArgv` (rename not in typed parser) | URLs redacted before constructing the op. |
| `config_get`/`config_set`/`config_unset` | `ConfigGet` / `ConfigSet` / `ConfigUnset` (local-only) | Key validated against `CONFIG_KEY_ALLOWLIST` and `CONFIG_DENIED_KEY_PATTERNS`. |
| `reset_soft`/`reset_mixed`/`reset_hard`/`reset_merge`/`reset_keep`/`reset_paths` | `ResetSoft` / `ResetMixed` / `ResetHard` / `ResetMerge` / `ResetKeep` / `Reset` (with `ResetMode::Mixed`) | Destructive operations. |
| `clean_preview` | subprocess `git clean -n -d`, parsed into `CleanPreview { entries: Vec<CleanEntry> }` | `CleanEntry` carries path + `CleanEntryKind` (`File \| Directory \| IgnoredFile \| IgnoredDirectory`). |
| `clean(exec, repo, req: CleanRequest)` | `ManagedGitArgv` for `git clean -f [-d] [-x]` | `CleanRequest::is_broad()` rejects `ignored=true` at root — caller must enforce. |
| `push_permission_hint(&PushRequest)` | — | Returns a state-aware description ("push (delete remote branch)", "push (force — destructive, denied by default)"). |
| `describe_network_operation(&GitOperation)` | — | Returns a one-line summary for permission prompts. |

### Config allowlist (`src/git_network_ops.rs`)

`CONFIG_KEY_ALLOWLIST` lists safe local-scope key prefixes (`branch.`, `pull.rebase`, `rebase.autosquash`, `commit.gpgsign`, `core.autocrlf`, `http.postbuffer`, `http.sslverify`, etc.). `CONFIG_DENIED_KEY_PATTERNS` rejects anything in `credential.*`, `http.*`, `url.*`, `core.gitProxy`, `core.sshCommand`, `core.sshVariant`. `validate_config_key(key, allow_local_only)` blocks global-only keys (`user.*`, `gpg.format`) when `allow_local_only=true`.

### Destructive policy

`reset_hard`/`reset_merge`/`reset_keep`/`clean` are tagged as `DestructiveWorktree` or `DestructiveHistory` risk by the parser, so command-intent routing carries `ExecutionCapability::DestructiveFileMutation` (default: `Deny`). The tool path (typed mutation API) does NOT enforce this — it trusts the model's permission flow. The `clean` mutation rejects `is_broad()` (ignored + no paths) at the tool dispatch layer with a `ToolError::Execution`.

### Projector additions (`src/git_mutation_projector.rs`)

* `project_network_mutation(&MutationResult)` — wraps `project_mutation` and appends the captured git output ("From origin\n  abc..def  main -> origin/main") under a `network output:` section, with a byte-count fallback for large outputs.
* `project_destructive_mutation(&MutationResult)` — wraps `project_mutation` with an explicit recovery hint (`git reflog` + `git reset --hard <sha>`).

### Tool integration (`src/tool/git.rs`)

The `mutation` action enum gains 19 new entries:

```
fetch, pull, push,
remote_add, remote_remove, remote_set_url, remote_rename,
config_get, config_set, config_unset,
reset_soft, reset_mixed, reset_hard, reset_merge, reset_keep, reset_paths,
clean_preview, clean
```

New parameters: `remote`, `url`, `old_name`, `refspecs`, `all`, `prune`, `strategy`, `force_with_lease`, `force_push`, `set_upstream`, `key`, `value`, `scope`, `mode`, `dry_run`, `ignored`, `directories`.

`scope_unwrap_local` resolves the `scope` parameter to a local-only boolean (Phase E intentionally disallows `global` scope via the tool — those writes belong in `~/.gitconfig` outside the repo boundary, and the dispatcher logs a WARN when a non-local scope is requested).

### Schema additions (`crates/codegg-config/src/schema.rs`)

`CommandIntentConfig` gains two fields:

* `route_git_network: Option<RouteLevel>` — gates command-intent routing of `git fetch`/`pull`/`push`/`remote`/`config` to the Git backend. Default `off`.
* `route_git_destructive: Option<RouteLevel>` — gates command-intent routing of `git reset --hard`/`reset --merge`/`reset --keep`/`clean`. Default `off`.

Tool-level typed actions (the model-facing `git` tool API) are unaffected by these flags — they are routed via the dedicated `mutation` action regardless of routing mode.

### Permission defaults (`src/command_intent/plan.rs`)

The existing per-capability defaults already cover Phase E without modification:

* `Network` → `PermissionDefault::Ask` (Medium risk).
* `DestructiveFileMutation` → `PermissionDefault::Deny` (High risk). Hard reset and broad clean are blocked at the routing gate.
* `GitMutation` → `PermissionDefault::Ask` (Medium risk) for non-`add` subcommands; `git add` is the only `Allow` short-circuit.

### Tests

`tests/git_network_integration.rs` covers (23 tests, all green):

* URL redaction — anonymous, `user:password@`, bare `user@`.
* Network failure classification — DNS, authentication, ref rejected, timeout.
* Remote management — add (with credential redaction), rename, remove.
* Config allowlist — denies `credential.helper` and `user.name` (global-only), allows `pull.rebase` and `rebase.autosquash` round-trips.
* Network round-trips on local bare-remote fixture — `fetch` from pushed commit, `push` with `set_upstream`, `pull --ff-only`.
* Destructive — `reset_hard` discards uncommitted + history, `reset_soft` keeps worktree, `clean_preview` lists untracked, `clean` removes them, `CleanRequest::is_broad()` is enforced.

Tests skip gracefully when `git` is unavailable so CI on minimal containers still passes.

## Phase F — Conflicts, Recovery, Ergonomics, and Closure

Phase F turns in-progress repositories (merge, rebase, cherry-pick, revert, bisect, apply-mailbox, sequencer) and their conflicts into first-class structured data. It layers operation-aware recovery on top of the existing mutation framework, exposes the active state to the model and TUI, and consolidates the Git architecture across Phases A–E.

### Repository operation-state discovery (`egggit::operation_state`)

`crates/egggit/src/operation_state.rs` exposes a typed `RepositoryOperationState` enum that detects the active operation by inspecting `.git/` plumbing files. The previous `OperationState` (still re-exported for back-compat) only knew about five families; the new model covers eight:

| Variant | Sentinel | Notes |
|---------|----------|-------|
| `None` | — | Clean repository. |
| `Merge(MergeState)` | `MERGE_HEAD` | Carries `original_head`, `other_head`, `message`, `in_progress`. |
| `Rebase(RebaseState)` | `rebase-merge` / `rebase-apply` / `REBASE_HEAD` | Carries `original_head`, `current_head`, `upstream`, `onto_branch`, `current_step`, `total_steps`, `interactive`, `apply_mode`. |
| `CherryPick(SequenceState)` | `CHERRY_PICK_HEAD` | Original + current HEAD plus target SHA. |
| `Revert(SequenceState)` | `REVERT_HEAD` | Same. |
| `Bisect(BisectState)` | `BISECT_LOG` | Bad/good/remaining-trials snapshot. |
| `ApplyMailbox(ApplyState)` | `rebase-apply/{next,last}` (without `head-name`) | Distinguished from rebase by absence of `head-name`. |
| `Sequencer(SequencerState)` | `sequencer/todo` | Used by Git ≥2.25 for cherry-pick/revert/quote-rebase bookkeeping; carries `action`, `subject`, `current_step`, `total_steps`. |
| `Unknown(UnknownOperationState)` | Unrecognized sentinel | Sanitized description for forward-compat. |

Detection logic lives in `detect_repository_operation_state(git_dir: &Path)` and is exposed via `detect_operation_state_for_root(root: &Path)` which canonicalizes `.git` (handling linked-worktree `.git` files). The detection logic is pure filesystem inspection — no mutation.

`RepositoryOperationState::available_actions()` returns the recovery action set (`RecoveryAction::{Continue, Abort, Skip}`) that is legal for the active family. `RepositoryOperationState::action_available(action)` answers "is this action allowed right now?" so callers don't have to maintain a parallel allowlist.

### Conflict model (`egggit::conflict`)

`crates/egggit/src/conflict.rs` exposes a typed conflict model that does NOT auto-resolve anything. `RichRepoStatus.conflict_entries` (populated by `parse_status_v2`) carries one `ConflictEntry` per conflicted path with:

| Field | Type | Notes |
|-------|------|-------|
| `path` | `String` | Repo-relative path of the conflicted file. |
| `status_code` | `String` | Raw XY code (`uUU`, `UU`, `AA`, …). |
| `kind` | `ConflictKind` | Classified: `BothModified \| BothAdded \| BothDeleted \| AddedByUs \| AddedByTheirs \| DeletedByUs \| DeletedByTheirs \| Unknown`. |
| `shape` | `ConflictShape` | `File \| Rename \| Delete \| DirectoryReplacement \| Submodule \| UntrackedReplaced`. |
| `base / ours / theirs` | `ConflictObjectId` | SHA + mode per side; populated post-Phase D once we wire `git ls-files -u`. |
| `original_path` | `Option<String>` | Pre-rename path when `shape=Rename`. |
| `has_conflict_markers` | `bool` | NUL-byte-free text scan for `<<<<<<<` + `=======` + `>>>>>>>`. |
| `staged_resolved` | `bool` | True once the path appears on the staged side after `git add`. |
| `submodule` | `bool` | Set when the porcelain v2 entry carried a submodule status code. |
| `recommended_actions` | `Vec<RecommendedConflictAction>` | Conservative set (`edit markers`, `git add <path>`, `git checkout --ours`, `git checkout --theirs`, `git rm`). |

Helpers:

* `classify_conflict_code(xy)` — maps XY codes to `ConflictKind`.
* `buffer_contains_conflict_markers(text)` — NUL-free text marker detector.
* `looks_binary(bytes)` — NUL-byte heuristic for binary-vs-text decision.
* `default_actions_for(kind, shape)` — minimal-action set the agent should consider.
* `ConflictReport::from_entries(entries)` — aggregated summary (total / unresolved-with-markers / resolved / submodule-conflicts / all-resolved flag).

### Operation-aware recovery (`src/git_recovery.rs`)

`src/git_recovery.rs` adds typed helpers `continue_in_progress`, `abort_in_progress_typed`, and `skip_in_progress` that:

1. Call `detect_operation_state_for_root(repo_root)` to identify the active operation.
2. Build the matching typed `GitOperation` (e.g. `Merge { abort: true, .. }` for merge-abort, `Rebase { skip: true, .. }` for rebase-skip).
3. Re-check `state.action_available(action)` to refuse cross-operation misuse (`rebase --abort` while a merge is in progress).
4. Run the executor (snapshots + state-delta + RunStore).
5. Tag the result with `outcome` reflecting the action (e.g. `Completed` for a successful abort, `Conflict` if `git <op> --continue` returned 1 because of unresolved conflicts).

Family-specific guards:

* `Bisect`, `ApplyMailbox`, and `Unknown` refuse automatic recovery (the operator must drive them manually with `git bisect` / `git am --abort` / direct `.git` inspection).
* `Merge` only supports `Continue` / `Abort`; `Skip` is rejected at the precondition layer (git merge has no `--skip`).
* Sequencer-driven operations (`CherryPick`/`Revert` ≥ Git 2.25) funnel through the typed codegg-git variants rather than the generic `git operation --abort` placeholder.

The legacy `git_mutations_ops::abort_in_progress` is kept as a shim for backward compat but delegates to `git_recovery::abort_in_progress_typed`.

### Agent ergonomics — git tool schema (`src/tool/git.rs`)

The native `git` tool gains three new model-facing parameters:

* `operation_state: bool` (default `false`) — when `true`, the tool returns the typed active operation family plus conflicted paths and legal recovery actions. Mutually exclusive with `subcommand`, `mutation`, and `recover`.
* `recover: "continue" | "abort" | "skip"` — operation-aware recovery action. Mutually exclusive with `mutation` (enforced via field description; Phase F does not yet enforce at the JSON level).
* `operation_state` and `recover` are dispatched in `GitTool::dispatch_operation_state` and `GitTool::dispatch_recover` respectively.

`description()` is rewritten to advertise the parameters, the conflict-not-auto-resolved semantic, and the recovery-by-state semantic so the model reaches for the typed API instead of raw `git merge --abort` invocations.

A schema snapshot test module (`tool::git::schema_tests`) pins the top-level keys, the `mutation` enum size, the `recover` enum exactly-equal to `[continue, abort, skip]`, the presence of `operation_state`, and that the description mentions both recovery and conflicts.

### TUI integration (`src/tui/commands/git_sidebar.rs` + `src/tui/app/state/session.rs`)

`GitSidebarState` and `GitSidebarInfo` gain three new fields:

* `operation_state_label: Option<String>` — `None` when clean, `"merge"`/`"rebase"`/etc. otherwise.
* `available_actions: Vec<String>` — `["continue", "abort"]` for a merge, `["continue", "abort", "skip"]` for a rebase, etc.
* `conflicted_paths: Vec<String>` — repo-relative conflicted paths from `RichRepoStatus.conflict_entries`.

`TuiCommand::GitSidebarRefreshFinished` carries the new fields, and `super::super::commands::git_sidebar::apply_git_sidebar_refresh` writes them into the cached state. The background probe runs once (existing 3 s timeout, generation-safe), so render purity is preserved.

Sidebar update triggers (`TuiMsg::SelectSession`, session reload, after git mutations) automatically pick up the new fields — no render-path changes required.

### Projection closure (`src/git_mutation_projector.rs`)

`project_mutation()` already covers completed / conflict / fast-forward / rejected outcomes; `project_recovery()` adds an outcome-aware projection for `recover:*` runs. The new helper prints before/after HEAD + conflict counts, records the action and family, and tailors the next-step hint based on the outcome (`"operation aborted; repository back to clean state"` for a successful abort; `"resolve conflict markers, …, then re-run recover: continue"` for a still-conflicted continue).

The shell output projectors (`RawProjector`, `TruncatedProjector`, `ErrorRetentionProjector`, the existing Git native projectors) cover `git status`, `git diff`, and `git log`. Recovery is intentionally NOT routed through the shell projector pipeline — it produces typed `MutationResult` values that flow through `project_recovery`. This avoids projector drift and keeps recovery semantics consistent with the mutation framework.

### RunStore observability (`src/git_run_store.rs`)

Recovery operations are persisted via the new `persist_recovery()` helper. Each recovery run is `RunKind::GitMutation` with `PlannedBackend::Git`, `ActualBackend::Git`, `Ownership::DelegatedBackend`, and `backend.detail = "recover:<continue|abort|skip>"`. The detail tag is grep-able and stable for dashboards; existing mutation metrics consume it as a fixed-cardinality label without exposing per-path or per-ref dimensions.

### Compatibility cleanup

* `grep -n "GitMutating" src/` returns only the `PlannedBackend::GitMutating`/`ActualBackend::GitMutating` deprecated markers. Their use is restricted to legacy provenance migration; new code must use `PlannedBackend::Git`/`ActualBackend::Git`. This matches the Phase B unification.
* The legacy `git_mutations_ops::abort_in_progress` heuristically-guesses `git merge --abort` then `git rebase --abort`. Phase F replaces it with operation-aware `git_recovery::abort_in_progress_typed`. The legacy function remains as a deprecated shim for the bash fallback path until callers migrate.

### Security review (Phase F scope)

* **Path traversal / pathspec injection** — `RepoPath::new` continues to reject absolute paths, parent traversal, and NUL bytes. Recovery helpers invoke typed `GitOperation`s with `render_argv`, which places paths after `--` to avoid option smuggling.
* **Revision names beginning with `-`** — `revision` arguments are passed through `RevisionExpr::new` (rejects empty) and never interpolated into bare argv without a `--` separator.
* **Option smuggling around `--`** — Recovery operations for `cherry-pick`, `revert`, and `merge` build typed variants that render their argv through `codegg_git::render_argv`, which forces `--` before paths.
* **Repository root escape via `-C`, symlinks, submodules, worktrees** — All recovery paths run with `current_dir(repo_root)`; the root is resolved via `RepoRoot::new` which canonicalizes symlinks.
* **Hostile Git config and aliases** — Typed `GitOperation`s bypass user-side aliases because the parser never invokes git's `alias.*` resolution paths; only the typed variant is rendered.
* **External helpers / credential hooks** — The recovery executor reuses `GitEnvPolicy` (`GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`, `GIT_SEQUENCE_EDITOR=true`), preserving the Phase D hardening.
* **Editor / sequence-editor spawning** — Same as above.
* **SSH command / config injection** — Recovery operations do not introduce new network calls. SSH agent handling stays as in Phase E (`SSH_AUTH_SOCK`, `SSH_AGENT_PID` are part of the standard `ALLOWED_ENV_VARS` set).
* **Credential leakage** — `backend.detail` for recovery is a fixed low-cardinality label; it does not contain refs, paths, or URLs.
* **Force / destructive misclassification** — `RecoveryAction::Abort` is tagged `HistoryIntegration + DestructiveHistory` by `risk_classes_for_recovery()` (see `src/git_recovery.rs`), so command-intent routing carries `DestructiveFileMutation` capability → `PermissionDefault::Deny` for high-risk cases.
* **No-double-execution guarantees** — `recover` and `operation_state` are mutually exclusive at the dispatch layer (one or the other runs per tool call). The mutation tool path also persists the run to RunStore after the action completes.
* **Race conditions between precondition snapshot and mutation** — `git_recovery::run_recovery` snapshots before running the mutation, captures the post-action snapshot, and refuses to run when the post-action state no longer reflects the requested action.

### Performance and resource review

* Status / sidebar refresh latency — Probe continues to use a single `git status --porcelain=v2 -z --branch` call plus a filesystem `.git` read; the recovery path adds one extra `git <op> --abort|--continue|--skip` invocation, no additional probes.
* Process count per operation — Unchanged from Phase E.
* Large repository status/diff behavior — Phase F does not introduce new subprocess fan-out. Sidebar generation gates (timeout 3 s, generation-safe) are unchanged.
* RunStore overhead — One additional row per recovery action; same `RunKind` and `Ownership` as standard mutations so cardinality stays bounded.
* Projection memory — `project_recovery()` produces ≤1 KiB; same memory bounds as `project_mutation()`.

### Cross-platform review

* Path encoding — Same path handling as Phase E (`RepoPath`, `RepoRoot`). NUL-byte detection still uses byte checks; UTF-8 decoding only happens at projector time.
* Executable discovery — Recovery `GitMutationExecutor` inherits the Phase D `ALLOWED_ENV_VARS` policy (PATH, HOME, etc.).
* Process termination — `kill_on_drop(true)` is preserved on the `Command` built by `GitEnvPolicy::apply`.
* HOME / XDG — Phase F does not change credential-helper behavior; the existing `ALLOWED_ENV_VARS` set includes the relevant XDG variables.
* SSH agent — `SSH_AUTH_SOCK`, `SSH_AGENT_PID` preserved.
* Temp repository fixtures — Existing helpers in `tests/git_mutations_integration.rs` continue to work; Phase F adds `tests/git_recovery_integration.rs` with 18 tests.
* Symlink behavior — `RepoRoot::new` canonicalizes; tests use real tempdirs, not symlinks.
* Newline and NUL parsing — `buffer_contains_conflict_markers` is UTF-8 text only; binary detection uses a NUL-byte heuristic so we never pattern-match on NUL-as-string-content.

### Tests added

| Path | Tests | Coverage |
|------|-------|----------|
| `crates/egggit/src/operation_state.rs` | 7 | None / Merge / Rebase / Sequencer / CherryPick / Bisect / available_actions matrix. |
| `crates/egggit/src/conflict.rs` | 6 | classify_conflict_code / conflict marker detection / binary detection / default action policy / report aggregation. |
| `crates/egggit/src/status_v2.rs` | 1 (extended) | `conflict_entries` and `conflict_report` populated from porcelain v2. |
| `src/git_recovery.rs` | 4 | State-action matrix for the four sentinel families. |
| `src/tool/git.rs::schema_tests` | 6 | Top-level keys + mutation enum + recover enum + description + drift guards. |
| `src/git_mutation_projector.rs` | 3 | project_recovery happy path / conflicts path / abort-completed path. |
| `tests/git_recovery_integration.rs` | 18 | End-to-end on tempdir fixtures: continue with conflicts, continue after resolution, abort without state, abort in progress, skip during rebase, etc. |
| `src/tui/app/state/session.rs` | 3 (extended) | Sidebar info with operation-state fields. |
| `tests/tui_render.rs` | 99 (existing) | Validated against new GitSidebarInfo field set. |

### Phase F corrective security closure

Two Phase F security-review findings were resolved post-merge by the corrective closure pass:

1. **`remote_set_url` credential leakage** — `GitOperation::RemoteAdd.url` and `GitOperation::RemoteSetUrl.url` are now typed as `codegg_git::RedactedUrl` (a newtype carrying both raw and redacted forms), instead of `String`. `Debug`, `Display`, `Serialize`, and any externally observable surface see only the redacted form. The raw form is reachable exclusively through `RedactedUrl::expose_secret()`, which is consumed exclusively at the final `render_argv` boundary. `remote_add()` and `remote_set_url()` in `src/git_network_ops.rs` wrap the incoming URL via `RedactedUrl::new(url)` before constructing the typed operation; `MutationResult` produced by both helpers flows through `sanitize_truncate_for_result` in `src/git_mutations.rs`, which applies `redact_url_credentials_in_text` to stdout/stderr before they reach `RunStore`. The RunStore audit log additionally flows through `sanitize_argv_for_run_store` (in `src/git_network_policy.rs`), which redacts URL-bearing tokens in the persisted `command`/`argv` fields without affecting the rerun descriptor's raw argv (the rerun path needs the raw URL for re-execution to authenticate).

2. **Raw fallback path missing hardened env policy** — Every Codegg-owned `git` subprocess now flows through `GitEnvPolicy::apply()` (tokio async) or the new `GitEnvPolicy::apply_sync()` (synchronous TUI probes). The policy's default includes `strip_command_bearers = true`, which removes `GIT_ASKPASS`, `GIT_SSH_COMMAND`, `GIT_PROXY_COMMAND`, all `GIT_CONFIG_*` injection vectors, `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_PAGER`, and `PAGER` from the inherited environment. Affected callers: `src/tool/git.rs::run_raw_subcommand`, `src/git_service.rs::run_git_raw`, `src/tool/commit.rs::fetch_head_message`, `src/core/daemon.rs::SnapshotWorkspace`, the TUI `handle_diff_command` / `handle_revert_command`, and `crates/codegg-core/src/worktree.rs::create_worktree` / `remove_worktree` (the worktree crate keeps a local mirror because `codegg-core` cannot depend on root-crate helpers).

The two-stage `apply`/`apply_sync` split ensures both the TUI's synchronous dialog probes and the daemon's async subprocess path share the exact same allowlist (`ALLOWED_ENV_VARS`) and hard-deny set (`ALWAYS_STRIPPED_ENV_VARS`, expanded to 27 vars in the closure). See `docs/validation/git-security-review.md` for the full resolution notes and `architecture/git_phase_f_handoff.md` for the original Phase F review context.

### Shell boundary honesty

The corrective closure pass established a hard invariant between the **direct Git execution path** and the **shell-owned execution path**:

- **Direct Git execution** (typed `GitMutationExecutor`, BashTool → Git backend, managed argv fallback, raw subcommand fallback) flows through `GitEnvPolicy::apply()` / `apply_sync()`. The resulting run is tagged `PlannedBackend::Git`, `ActualBackend::Git`, `RunOwnership::DelegatedBackend`, and the RunStore audit argv is redacted via `sanitize_argv_for_run_store`.
- **Shell-owned Git expressions** (anything with pipes, redirects, command substitution, semicolons, env assignments, or quoted glob patterns) are NOT silently rewritten as `ActualBackend::Git`. They remain `ActualBackend::RawShell` and inherit the shell execution policy (`src/shell/policy.rs`), which has its own redaction boundary (`src/shell/redactor.rs::apply_redaction_hook`) and command-classification rules (`src/command_intent/shell_shape.rs`).

The classification boundary is `src/command_intent/shell_shape.rs::parse_shell_words` + `has_shell_operators()`. When parsing succeeds and no operators are detected, the command is eligible for active Git routing; when parsing fails or operators are present, the command falls back to raw shell. This separation prevents commands like `git push && rm -rf .` from being misrepresented as a `Git` execution just because the leading token is `git`.

Operators that disqualify direct Git routing:

- `|`, `;`, `&`, `&&`, `||`
- `$`, `${`, `$(`, `` ` ``
- `>`, `<`, `>>`
- newlines, NUL bytes, quotes paired across the line

The Rust tool surface (typed mutations, recovery, raw fallback) and the BashTool translation layer both honor this boundary — they never upgrade a shell-owned command to `ActualBackend::Git`. RunStore provenance reflects the actual executor, not the intent family.

### Polish / maintainability / verification closure

The corrective security closure (`cb192e9`, `c2e806f`, `53b2beb`) left three
post-closure invariants that the polish pass tightened without
changing runtime behavior:

1. **Canonical subprocess policy.** `ALLOWED_ENV_VARS` and
   `ALWAYS_STRIPPED_ENV_VARS` now live in
   [`crates/codegg-git/src/process_policy.rs`](crates/codegg-git/src/process_policy.rs)
   (the single source of truth). Both the root crate
   (`src/git_mutations.rs::GitEnvPolicy`) and `codegg-core`
   (`crates/codegg-core/src/worktree.rs::hardened_git_command`) consume
   the same lists, so they cannot silently drift. Drift is caught by
   `cargo test -p codegg-core` (`worktree_uses_canonical_policy`,
   `canonical_includes_locally_drifted_entries`) and
   `src/git_mutations.rs::policy_drift_tests`.
2. **Audit-safe rerun argv.** `RerunDescriptor.argv` is
   `Option<AuditSafeArgv>` (newtype in
   [`crates/codegg-git/src/sensitive.rs`](crates/codegg-git/src/sensitive.rs)).
   The only construction path (`AuditSafeArgv::from_argv`) runs the
   URL sanitizer on every token, so durable RunStore records are
   credential-free. The deserializer re-runs the sanitizer to
   normalize historical records. See
   [`docs/validation/git-rerun-secret-lifecycle.md`](docs/validation/git-rerun-secret-lifecycle.md)
   for the lifecycle inventory.
3. **Forbidden-pattern static checks.**
   [`scripts/check_git_forbidden_patterns.py`](scripts/check_git_forbidden_patterns.py)
   enforces (a) `expose_secret()` only at the `render_argv`
   boundary, (b) no hand-maintained env-policy tables, (c)
   `RerunDescriptor.argv` is always `AuditSafeArgv`, (d) git argv
   flowing into `RunInvocation` is sanitized. The script is part of
   the standard local validation.

Verified state, the execution-origin matrix, and remaining
limitations are recorded in
[`architecture/git_polish_verification_handoff.md`](architecture/git_polish_verification_handoff.md).


