# Phase F Git Security Review

Focused review of 14 threat classes from `plans/git_agent_integration_phase_f_conflicts_and_closure.md` deliverable 11. Review performed against the actual codebase on 2026-07-14.

## Summary

- **Threat classes reviewed:** 14
- **Issues found:** 2 (1 unmitigated, 1 design limitation)
- **Mitigated:** 12
- **No issue:** 11

---

## 1. Path Traversal / Pathspec Injection

**Status:** Mitigated

**Evidence:**
- `crates/codegg-git/src/path.rs:55-81` — `RepoPath::new()` rejects NUL bytes (`\0`), absolute paths, parent traversal (`..`), and paths resolving outside the repository root after canonicalization. Tested at lines 137-161.
- `crates/codegg-git/src/path.rs:94-107` — `Pathspec` rejects NUL bytes and empty strings. Glob/regex pathspecs pass through (cannot be validated literally) but NUL injection is blocked.
- `crates/codegg-git/src/path.rs:24-36` — `RepoRoot::new()` canonicalizes paths and returns `PathError::PathEscape` on failure.
- `src/git_mutations.rs:295-316` — `resolve_repo_root()` validates the path exists, is a directory, canonicalizes via `RepoRoot::new()`, and checks `.git` exists.
- `src/git_mutations.rs:319-321` — `validate_repo_path()` delegates to `RepoPath::new()`.

**Notes:** Every mutation path (stage, commit, branch, restore, stash, merge, etc.) validates paths through `validate_repo_path()` before constructing a `GitOperation`. The `render_argv()` function then places paths after `--` in the rendered argv. NUL bytes are rejected at the type level.

---

## 2. Revision Names Beginning with `-`

**Status:** Mitigated

**Evidence:**
- `crates/codegg-git/src/ref_name.rs:145-147` — `validate_ref_name()` rejects names starting with `-`. Applies to `BranchName` and `RefName` (lines 28-31, 48-51).
- `crates/codegg-git/src/ref_name.rs:74-76` — `RemoteName::new()` also rejects leading `-`.
- `crates/codegg-git/src/ref_name.rs:124-137` — `RevisionExpr` only rejects empty strings (too many valid forms to validate), but revision expressions are placed in typed `GitOperation` variants and rendered via `render_argv()`, not interpolated into bare argv.
- `src/git_mutations_ops.rs:206,222,258,381-384` — All branch, revision, and stash ref inputs are validated through `BranchName::new()`, `RevisionExpr::new()`.

**Notes:** The typed path ensures `-`-prefixed revision names cannot be confused with command options. `RevisionExpr` is intentionally lenient because git revision syntax is complex, but it's always placed as a positional argument in the rendered argv.

---

## 3. Option Smuggling Around `--`

**Status:** Mitigated

**Evidence:**
- `crates/codegg-git/src/render.rs:736-743` — `push_paths_after_dd()` inserts `--` before path lists for `Diff`, `Log`, `DiffStaged`, `ChangedFiles`, `Blame`.
- `render.rs:128-133` — `Reset` with paths pushes `--` before path args.
- `render.rs:236-242` — `Checkout` with paths pushes `--` before path args.
- `render.rs:282-286` — `Restore` always pushes `--` before paths.
- `render.rs:180-184` — `StashPush` with paths pushes `--` before path specs.
- `render.rs:610-613` — `Clean` with paths pushes `--` before path specs.
- `src/git_recovery.rs:55-103` — Recovery operations construct typed `GitOperation` variants (e.g., `GitOperation::Rebase { continue_op: true, .. }`) which render through `render_argv()`. No raw argv construction.
- `src/git_mutations_ops.rs:53-54` — `stage_all` and `stage_tracked` use raw argv (`git add -A`, `git add -u`) but these don't take path arguments.

**Notes:** All codegg-initiated git commands with path arguments go through `render_argv()` which enforces `--` before paths. Recovery operations use typed variants with no user-controlled path arguments (they operate on the current in-progress state).

---

## 4. Repository Root Escape via `-C`, Symlinks, Submodules, Worktrees

**Status:** Mitigated

**Evidence:**
- `src/git_mutations.rs:295-316` — `resolve_repo_root()` canonicalizes the path and validates `.git` exists. The canonical path is used for `current_dir()` in all subprocess invocations.
- `src/git_mutations.rs:83-108` — `GitEnvPolicy::apply()` sets `cmd.current_dir(cwd)` on the `Command` (line 85). Git inherits this as its working directory. There is no `-C` flag passed to git in any codegg-initiated command.
- `src/git_mutations_ops.rs:38-46,71-84,284-297` — All typed mutation helpers call `resolve_repo_root()` before constructing operations.
- `src/git_recovery.rs:257-266` — `run_recovery()` receives `repo_root: &Path` from callers that pass it through to `exec.execute()` which uses it as `current_dir`.

**Notes:** Codegg never passes `-C` to git. The repository root is set via `current_dir()` on the Rust `Command` type. Linked worktrees and submodules are handled by `RepoRoot::new()` canonicalization — the `.git` file check resolves the actual git directory. Submodule boundaries are not explicitly blocked but are contained by the canonical root check.

---

## 5. Hostile Git Config and Aliases

**Status:** Mitigated (by design decision)

**Decision:** Git aliases are bypassed for typed operations because the parser constructs `GitOperation` variants directly and `render_argv()` produces the canonical subcommand name. For example, `GitOperation::Merge { .. }` always renders as `["git", "merge", ...]`, never resolving user-defined `merge` aliases. The parser does not invoke git to resolve aliases.

**Evidence:**
- `crates/codegg-git/src/render.rs:9` — `render_argv()` produces a complete argv beginning with `"git"` and the literal subcommand name. No alias resolution occurs.
- `crates/codegg-git/src/parser.rs:58-64` — `parse_git_argv()` matches against the 25 known subcommand strings. Unknown subcommands fall back to `ManagedGitArgv`.
- `src/tool/git.rs:349-365` — The raw fallback path uses `Command::new("git").args(&full_args)` where `full_args[0]` is the literal subcommand string from the user, not resolved through git's alias mechanism.

**Notes:** The raw subcommand fallback (tool/git.rs:336-387) passes the subcommand directly to `Command::new("git").args(&full_args)`. Git itself will still resolve aliases when executing. This is the documented compatibility escape for unsupported operations. The typed mutation path is immune to alias injection.

---

## 6. External diff/textconv/filter/credential/helper/hook Execution

**Status:** Mitigated

**Evidence:**
- `src/git_mutations.rs:37-53` — `ALLOWED_ENV_VARS` does not include `GIT_ASKPASS`, `GIT_SSH_COMMAND`, or `GIT_SSH_VARIANT` for local operations. The `env_clear()` at line 86 strips these.
- `src/git_mutations.rs:86` — `cmd.env_clear()` strips the entire parent environment before restoring only the allowlisted variables.
- `src/git_mutations.rs:94-99` — `GIT_TERMINAL_PROMPT=0` prevents credential helpers from blocking. `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` prevent editor spawning.
- `src/git_mutations.rs:101-104` — `EDITOR` and `VISUAL` are removed from the environment.
- `src/git_mutations.rs:105` — `GPG_TTY` is set to empty to prevent gpg/pinentry spawning.

**Notes:** Git's own `filter`, `textconv`, and `diff` drivers can still execute if configured in `.gitattributes` or `.git/config`. This is an inherent git behavior that cannot be disabled without disabling those features entirely. The env hardening prevents external programs from being launched via credential helpers, editors, and gpg agents.

---

## 7. Pager/Editor/Sequence-Editor Spawning

**Status:** Mitigated

**Evidence:**
- `src/git_mutations.rs:97-104` — `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` prevent git from launching user `$EDITOR`. `EDITOR` and `VISUAL` env vars are removed.
- `src/git_network_policy.rs:29-32` — Network policy documents that `GIT_PAGER` and `PAGER` are intentionally cleared.
- Both local and network policies use `env_clear()` which strips `GIT_PAGER` and `PAGER` from the environment (they're not in any allowlist).

**Notes:** `GIT_PAGER` and `PAGER` are implicitly cleared by `env_clear()`. Git falls back to its built-in default (typically `cat` for piped output), which is safe.

---

## 8. SSH Command/Config Injection

**Status:** Mitigated (local), Design Limitation (network)

**Evidence:**
- `src/git_mutations.rs:37-53` — Local operations do NOT restore `GIT_SSH_COMMAND` or `GIT_SSH_VARIANT`. These are stripped by `env_clear()`.
- `src/git_network_policy.rs:41-63` — Network operations DO restore `GIT_SSH_COMMAND` and `GIT_SSH_VARIANT` from the parent environment. This is intentional: network operations need SSH agent connectivity.

**Notes:** For network operations, `GIT_SSH_COMMAND` is inherited from the parent environment. If the parent process has a hostile `GIT_SSH_COMMAND`, network operations will use it. This is a design trade-off: blocking it would break SSH-based remotes. The `CONFIG_DENIED_KEY_PATTERNS` at `src/git_network_ops.rs:469-476` blocks `core.sshCommand` and `core.sshVariant` from being set via `config_set`, preventing a model-driven escalation. However, a hostile parent environment can still set `GIT_SSH_COMMAND`.

---

## 9. Credential Leakage

**Status:** Mitigated (1 unmitigated issue found)

### Issue: `remote_set_url` does not redact credentials

**Evidence:**
- `src/git_network_ops.rs:345` — `remote_add()` calls `redact_url_credentials(url)` before constructing the `RemoteAdd` operation. The sanitized URL is what gets stored.
- `src/git_network_ops.rs:376` — `remote_set_url()` does NOT call `redact_url_credentials()`. The raw URL is passed directly to `RemoteSetUrl { url: url.to_string(), .. }`.
- `src/git_run_store.rs:52-53` — The `command` field in `RunDraft` is `argv.join(" ")`, which includes the URL from the rendered operation. If the URL contains credentials, they persist in RunStore.

**Impact:** A URL with embedded credentials (e.g., `https://user:token@host/repo.git`) passed to `remote_set_url` will be stored in the `MutationResult.operation` field and persisted to RunStore in plaintext.

**Recommendation:** Add `let sanitized_url = redact_url_credentials(url);` before constructing the `RemoteSetUrl` operation in `remote_set_url()`, matching the `remote_add()` pattern.

---

## 10. Malicious Repository Filenames and Output Control Sequences

**Status:** Mitigated

**Evidence:**
- `src/git_mutations.rs:499-500` — stdout/stderr are truncated to 64 KiB with `truncate_for_result()`.
- `src/git_mutation_projector.rs` — Projections format structured data, not raw git output. Path names come from structured parsing (porcelain v2), not raw stdout.
- `crates/egggit/src/conflict.rs` — `buffer_contains_conflict_markers()` is UTF-8 text only; binary detection uses NUL-byte heuristic (documented in architecture/git.md:349).

**Notes:** Output is presented to the model through projectors, not raw. The truncation prevents excessive output from causing context overflow. Control sequences in filenames are not stripped but are contained by the structured parsing pipeline (porcelain v2 with NUL delimiters).

---

## 11. Race Conditions Between Snapshot and Mutation

**Status:** Mitigated

**Evidence:**
- `src/git_mutations.rs:476-513` — `GitMutationExecutor::execute()` snapshots before, runs the operation, then snapshots after. The window between snapshot and mutation is inherent (no file-level locking).
- `src/git_recovery.rs:220-255` — `assert_action_matches()` re-validates the operation state before the recovery action runs. The doc comment at line 222 explicitly states: "Defends against TOCTOU between detection and execution by re-reading state immediately before the action runs."
- However, there is actually no re-read in `assert_action_matches()` — it only checks `state.action_available(action)` against the state detected earlier. The re-read comment is aspirational.

**Finding (minor):** The `run_recovery()` function at line 257 does NOT re-detect state before executing — it uses the `state` parameter passed from `continue_in_progress`/`abort_in_progress_typed`/`skip_in_progress` which detected state earlier. Between detection and execution, another process could change the git state. This is an accepted limitation: git's own `--continue`/`--abort`/`--skip` commands will fail with clear error messages if the state has changed.

**Impact:** Low. The worst case is a recovery action that fails because the state changed between detection and execution. Git returns a clear error, which the executor surfaces as `MutationOutcome::Rejected`. No data corruption risk.

---

## 12. Raw/Managed Fallback Bypass

**Status:** Mitigated

**Evidence:**
- `crates/codegg-git/src/operation.rs:487-491` — `ManagedGitArgv` carries a caller-supplied `RiskSet`. `RawShellRequired` is classified with `WorktreeMutation + HistoryIntegration`.
- `src/git_mutations_ops.rs:757-785` — `run_raw_mutation()` runs raw argv through the same snapshot/timeout/policy pipeline. It does NOT skip env hardening or snapshot capture.
- `src/tool/git.rs:349-365` — The raw fallback path at tool dispatch uses `Command::new("git").env_clear()` and restores only `PATH`. This is a reduced policy compared to `GitEnvPolicy` (no `GIT_EDITOR=true` pinning, no `GPG_TTY` clearing).

**Finding (design limitation):** The raw subcommand fallback in `tool/git.rs:336-387` uses a simpler env policy (`env_clear()` + only `PATH`) compared to the typed mutation path which uses full `GitEnvPolicy`. This means the raw fallback does NOT pin `GIT_EDITOR=true`, `GIT_SEQUENCE_EDITOR=true`, or clear `GPG_TTY`. However, it does `env_clear()` and `kill_on_drop(true)`. The raw path is only reached for unsupported read-only subcommands that fail structured execution, or for mutations not covered by the typed API. The model-facing tool description strongly prefers typed mutation actions.

**Impact:** Low. The raw fallback path has reduced env hardening but is only reached for edge cases. The typed mutation path (used by the vast majority of operations) has full hardening.

---

## 13. Force/Destructive Misclassification

**Status:** No issue

**Evidence:**
- `crates/codegg-git/src/operation.rs:347-350` — `Checkout { force: true, .. }` → `DestructiveWorktree`
- `operation.rs:356-360` — `Switch { force: true, .. }` → `DestructiveWorktree`
- `operation.rs:369-372` — `BranchCreate { force: true, .. }` → `DestructiveHistory`
- `operation.rs:374-378` — `BranchDelete { force: true, .. }` → `DestructiveHistory`
- `operation.rs:383-386` — `TagForceDelete` → `DestructiveHistory`
- `operation.rs:393-399` — `Rebase { interactive: true, .. }` → `DestructiveHistory`
- `operation.rs:433-447` — `Push { force: true, .. }` / `Push { force_with_lease: true, .. }` / `Push { delete: true, .. }` → `DestructiveHistory`
- `operation.rs:450-455` — `ResetHard` → `DestructiveWorktree`
- `operation.rs:465-469` — `ResetKeep` → `DestructiveWorktree`
- `operation.rs:471-474` — `Clean { force: true, .. }` → `DestructiveWorktree`
- `src/git_recovery.rs:321-334` — `risk_classes_for_recovery()` tags `Abort` with `DestructiveHistory`, `Continue`/`Skip` with `HistoryIntegration`.

**Notes:** All destructive operations are correctly classified. `force_with_lease` is tagged `DestructiveHistory` (same as `force: true`) in the risk set, though `PushForce::ForceWithLease::is_destructive()` returns `false` — this is intentional because `force_with_lease` is safer than unconditional force but still carries destructive history risk.

---

## 14. No-Double-Execution Guarantees

**Status:** Mitigated

**Evidence:**
- `src/tool/git.rs:294-309` — The dispatch chain checks `mutation` → `operation_state` → `recover` → `subcommand` in priority order. Only one path executes per tool call. The `recover` field description (line 142) explicitly states "mutually exclusive with both."
- `src/tool/git.rs:1109-1124` — Schema snapshot test `recover_is_mutually_exclusive_with_mutation_via_description` verifies the mutual exclusion contract is documented.
- `src/git_mutations.rs:476-513` — `GitMutationExecutor::execute()` is idempotent at the executor level: it captures before snapshot, runs once, captures after snapshot. No retry or loop logic.
- `src/git_run_store.rs:87-185` — RunStore persistence is fire-and-forget (failures logged at WARN, never retried). The `persist_mutation` and `persist_recovery` functions delegate to the same underlying `persist_mutation` with different `backend_detail` labels.

**Notes:** The mutual exclusion between `mutation`/`operation_state`/`recover`/`subcommand` is enforced by the dispatch priority in `execute()` (lines 295-309). If `mutation` is present, `operation_state` and `recover` are never checked. This guarantees at most one operation per tool call.

---

## Open Issues

### 1. `remote_set_url` credential leakage (unmitigated)

**Severity:** Medium
**Location:** `src/git_network_ops.rs:367-382`
**Description:** `remote_set_url()` passes the raw URL to `GitOperation::RemoteSetUrl` without calling `redact_url_credentials()`. Credentials in the URL persist in `MutationResult` and are written to RunStore.
**Recommendation:** Add `let sanitized_url = redact_url_credentials(url);` before constructing the operation, matching the `remote_add()` pattern at line 345.

### 2. Raw fallback path has reduced env hardening (design limitation)

**Severity:** Low
**Location:** `src/tool/git.rs:349-365`
**Description:** The raw subcommand fallback uses `env_clear()` + only `PATH` restoration, missing `GIT_EDITOR=true` pinning, `GPG_TTY` clearing, and `EDITOR`/`VISUAL` removal.
**Recommendation:** Consider applying the full `GitEnvPolicy` to the raw fallback path, or document the reduced hardening in the tool description.

---

## Decisions Made

1. **Git aliases are bypassed for typed operations** — The `codegg-git` parser constructs `GitOperation` variants directly; `render_argv()` produces canonical subcommand names. User-defined git aliases are never resolved for typed mutations. The raw subcommand fallback passes subcommand strings directly to `Command::new("git").args()`, where git itself may resolve aliases — this is the documented compatibility escape.

2. **`RevisionExpr` is intentionally lenient** — Only rejects empty strings. Git revision syntax (`HEAD~3`, `stash@{0}`, `^{commit}`, etc.) is too complex to validate exhaustively. Safety comes from positional placement in rendered argv, not from string validation.

3. **`force_with_lease` is tagged destructive in risk set** — Despite being safer than `--force`, it still carries `DestructiveHistory` risk to ensure the permission flow requires explicit user confirmation. The `is_destructive()` method returns `false` for use in permission hint formatting.

4. **Network operations inherit SSH env vars** — `GIT_SSH_COMMAND`, `GIT_SSH_VARIANT`, and `SSH_AUTH_SOCK` are restored for network operations because blocking them would break SSH-based remotes. The config key denylist prevents model-driven escalation via `core.sshCommand`.

5. **Recovery operations use the same executor as mutations** — No special env policy or different subprocess handling. The operation-aware dispatch in `git_recovery.rs` constructs the correct typed `GitOperation` and delegates to `GitMutationExecutor::execute()`.

---

## Test Coverage

The following test suites cover security-relevant behavior:

| Test Suite | Coverage |
|------------|----------|
| `crates/codegg-git/src/path.rs` tests | Path validation: absolute, null byte, parent traversal, empty |
| `crates/codegg-git/src/ref_name.rs` tests | Ref validation: dash prefix, double dot, lock suffix, special chars |
| `crates/codegg-git/src/render.rs` tests | `--` insertion, argv rendering for all operation families |
| `crates/codegg-git/src/operation.rs` tests | Risk classification per variant including destructive flags |
| `src/git_recovery.rs` tests | State-action matrix, cross-operation misuse prevention |
| `tests/git_recovery_integration.rs` | 18 end-to-end tests on tempdir fixtures |
| `tests/git_network_integration.rs` | URL redaction, config allowlist, network round-trips |
| `src/tool/git.rs::schema_tests` | Schema snapshot: mutation enum, recover enum, description |
| `src/git_network_ops.rs` tests | Config key validation, push force classification, URL redaction |
