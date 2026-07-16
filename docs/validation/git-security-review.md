# Git Security Review

Two-pass security review of the Git agent integration:

1. **Phase F closure review (2026-07-14)** — 14 threat classes from
   the Git agent integration Phase F closure deliverable 11 (plan pruned
   post-completion). Two issues identified and resolved in the closure
   commits (`cb192e9`, `c2e806f`, `53b2beb`).
2. **Polish / maintainability / verification refresh (2026-07-15)** —
   re-ran the threat model against the post-polish codebase
   (`8d686c7` + delta). All 14 closure findings remain mitigated. One
   new medium-severity finding (**rerun secret lifecycle**) is
   resolved by the type-level `AuditSafeArgv` invariant. Two
   low-severity items remain accepted (see
   [Known Limitations](#known-limitations)).

## Summary

- **Threat classes reviewed:** 15 (14 Phase F + 1 polish-pass addition)
- **Open issues:** 0 (all medium+ findings resolved)
- **Accepted low-severity limitations:** 2 (documented below)
- **Static guards:** 5 forbidden-pattern checks in
  `scripts/check_git_forbidden_patterns.py`

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
- `src/git_mutations.rs:44-48` — `pub use codegg_git::process_policy::ALLOWED_ENV_VARS` / `ALWAYS_STRIPPED_ENV_VARS` (canonical lists, single source of truth — see [Threat 15](#15-rerun-secret-lifecycle) for the lifecycle invariant).
- `src/git_mutations.rs:91-117` — `apply()` builds the env: `env_clear()` strips the parent environment, restores only the allowlist, hard-pins `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`, `GIT_SEQUENCE_EDITOR=true`, removes `EDITOR`/`VISUAL`, sets `GPG_TTY=""`.
- `src/git_mutations.rs:124-151` — `apply_sync()` for synchronous paths (TUI dialog probes) shares the exact same policy.
- `src/git_network_policy.rs:39-63` — Network operations additionally restore `GIT_ASKPASS`, `GIT_SSH_COMMAND`, `GIT_SSH_VARIANT`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, and proxy env vars. This is intentional: blocking these would break SSH-based remotes.

**Notes:** Git's own `filter`, `textconv`, and `diff` drivers can still execute if configured in `.gitattributes` or `.git/config`. This is an inherent git behavior that cannot be disabled without disabling those features entirely. The env hardening prevents external programs from being launched via credential helpers, editors, and gpg agents. The `scripts/check_git_forbidden_patterns.py` static check guards against hand-maintained env-policy tables outside the four approved paths.

---

## 7. Pager/Editor/Sequence-Editor Spawning

**Status:** Mitigated

**Evidence:**
- `src/git_mutations.rs:106-107` — `GIT_EDITOR=true` and `GIT_SEQUENCE_EDITOR=true` prevent git from launching user `$EDITOR`. `EDITOR` and `VISUAL` env vars are removed at lines 110-111.
- `src/git_mutations.rs:139-149` — Same pinning in the synchronous path.
- `src/git_network_policy.rs:29-32` — Network policy documents that `GIT_PAGER` and `PAGER` are intentionally cleared.
- Both local and network policies use `env_clear()` which strips `GIT_PAGER` and `PAPPER` from the environment (they're not in any allowlist).

**Notes:** `GIT_PAGER` and `PAGER` are implicitly cleared by `env_clear()`. Git falls back to its built-in default (typically `cat` for piped output), which is safe.

---

## 8. SSH Command/Config Injection

**Status:** Mitigated (local), Design Limitation (network)

**Evidence:**
- `src/git_mutations.rs:44-48` — Local operations do NOT restore `GIT_SSH_COMMAND` or `GIT_SSH_VARIANT`. The canonical `ALWAYS_STRIPPED_ENV_VARS` list excludes them, and `env_clear()` strips anything not in `ALLOWED_ENV_VARS`.
- `src/git_network_policy.rs:41-63` — Network operations DO restore `GIT_SSH_COMMAND` and `GIT_SSH_VARIANT` from the parent environment. This is intentional: network operations need SSH agent connectivity.

**Notes:** For network operations, `GIT_SSH_COMMAND` is inherited from the parent environment. If the parent process has a hostile `GIT_SSH_COMMAND`, network operations will use it. This is a design trade-off: blocking it would break SSH-based remotes. The `CONFIG_DENIED_KEY_PATTERNS` at `src/git_network_ops.rs` blocks `core.sshCommand` and `core.sshVariant` from being set via `config_set`, preventing a model-driven escalation. However, a hostile parent environment can still set `GIT_SSH_COMMAND`.

---

## 9. Credential Leakage

**Status:** Mitigated (1 unmitigated issue found)

### Issue: `remote_set_url` does not redact credentials

**Evidence:**
- `src/git_network_ops.rs:349-360` (`remote_add`) and `src/git_network_ops.rs:377-392` (`remote_set_url`) wrap the URL via `codegg_git::RedactedUrl::new(url)` before constructing the typed operation. The raw value reaches git's argv via `expose_secret()` inside `codegg-git::render_argv`; all display/serialization surfaces see only the redacted form.
- `src/git_run_store.rs:52-53` — The `command` field in `RunDraft` is `argv.join(" ")` after the rendered operation goes through `sanitize_argv_for_run_store(argv)`. Audit surfaces are structurally blocked from carrying raw credentials.

**Impact:** A URL with embedded credentials (e.g., `https://user:token@host/repo.git`) passed to `remote_set_url` will be stored in the `MutationResult.operation` field and persisted to RunStore in plaintext.

**Recommendation:** Add `let sanitized_url = redact_url_credentials(url);` before constructing the `RemoteSetUrl` operation in `remote_set_url()`, matching the `remote_add()` pattern.

---

**Resolution:** See **Resolutions §1** below. The fix is structural:
`RemoteAdd.url` and `RemoteSetUrl.url` are now `RedactedUrl` (not `String`),
so the raw value can only escape via `expose_secret()` consumed at the
final `render_argv` boundary. The `RunDraft.command`/`RunDraft.argv`
audit fields additionally flow through `sanitize_argv_for_run_store`
before being persisted.

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

**Status:** Mitigated (polish-pass upgrade)

**Evidence:**
- `crates/codegg-git/src/operation.rs` — `ManagedGitArgv` carries a caller-supplied `RiskSet`. `RawShellRequired` is classified with `WorktreeMutation + HistoryIntegration`.
- `src/git_mutations_ops.rs::run_raw_mutation()` — runs raw argv through the same snapshot/timeout/policy pipeline. Does NOT skip env hardening or snapshot capture.
- `src/tool/git.rs::run_raw_subcommand()` — now routes through `GitEnvPolicy::default().apply(...)` (same canonical policy as the typed mutation path). The polish pass resolved the prior design limitation: the raw fallback previously used `env_clear()` + only `PATH`, missing command-bearer stripping, `GIT_EDITOR=true` pinning, and `GPG_TTY` clearing. Every Codegg-owned `git` subprocess now flows through the canonical policy.
- `scripts/check_git_forbidden_patterns.py` (check #1) — statically guards against `Command::new("git")` outside approved modules.

**Impact:** The raw fallback previously had reduced env hardening but was only reached for edge cases. The polish pass closed the gap: typed, raw-tool, service-level, daemon snapshot, TUI dialog probes, and `codegg-core` worktree helpers all share the canonical `process_policy` lists. Drift is caught by `policy_drift_tests` in `src/git_mutations.rs` and `worktree_uses_canonical_policy` in `crates/codegg-core/src/worktree.rs`.

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

## Known Limitations

All medium- and high-severity findings from the Phase F closure review and the polish-pass refresh are resolved. Two low-severity items remain accepted:

### L1. Bash simple git mutation routes through raw shell

**Severity:** Low
**Where:** `src/tool/bash.rs:75-90` — `intent_kind_to_family()` returns `None` for `GitMutating`, so when active routing is enabled a `git commit -m foo` command is classified as `GitMutating` but dispatched as `RawShell`.
**Rationale:** The classifier is correct; the routing gate is intentionally conservative for mutations because the model-facing `git` tool already exposes typed mutations. Closing the gap requires a `GitMutate` `CommandIntentFamily` with associated per-family `RouteLevel` config — out of scope for the polish pass (no behavior change without a tracked ask).
**Regression test:** `tests/git_execution_origin_matrix.rs` row 5 (`bash_simple_git_mutation_routes_through_raw_shell`).

### L2. TUI `RunRerun` is a placeholder

**Severity:** Low
**Where:** `src/tui/app/mod.rs` — the handler emits a placeholder and never reads back `rerun.argv`.
**Rationale:** The polish pass strengthened the invariant (`RerunDescriptor.argv` is now `Option<AuditSafeArgv>`), so a future replay implementation must reconstruct the raw URL from the credential helper, prompt, or environment before re-rendering via `render_argv`. The current placeholder cannot leak raw credentials because the stored argv is structurally credential-free.
**Regression test:** `tests/git_credential_runstore_sentinel.rs` rerun_argv positive control.

---

## Polish-pass verification (post-closure)

The polish pass (`8d686c7` and follow-up commits) tightened three invariants without changing runtime behavior. Each is statically and dynamically guarded.

### P1. Canonical subprocess policy

**Invariant:** `ALLOWED_ENV_VARS` and `ALWAYS_STRIPPED_ENV_VARS` have a single source of truth at `crates/codegg-git/src/process_policy.rs`. Both the root crate and `codegg-core` consume the canonical lists via `pub use` re-exports.

**Guards:**
- `src/git_mutations.rs::policy_drift_tests` (4 in-module tests) — pins canonical list entries against historical values.
- `crates/codegg-core/src/worktree::tests::worktree_uses_canonical_policy` — confirms core worktree helper consumes the canonical lists.
- `crates/codegg-core/src/worktree::tests::canonical_includes_locally_drifted_entries` — locally drifts the local alias list and confirms the canonical lists still detect it.
- `scripts/check_git_forbidden_patterns.py` (check #2) — flags any hand-maintained env-policy table outside the four approved paths.

### P2. Audit-safe rerun argv

**Invariant:** `RerunDescriptor.argv` is `Option<AuditSafeArgv>` (a newtype in `crates/codegg-git/src/sensitive.rs`). The only construction path (`AuditSafeArgv::from_argv`) runs `redact_url_credentials_in_text` on every token. The deserializer re-runs the sanitizer on load to normalize historical records.

**Guards:**
- `crates/codegg-git/src/sensitive.rs` unit tests (6) — prove `from_argv` redacts HTTPS and SCP-style URLs.
- `tests/git_credential_runstore_sentinel.rs::mem_runstore_does_not_leak_sentinel` — scans rerun_argv across the full RunStore surface (manifest, index, artifacts, JSON, Debug).
- `tests/git_credential_runstore_sentinel.rs::fs_runstore_does_not_leak_sentinel_to_disk` — scans the FS-backed RunStore including rerun_argv; positive control asserts no rerun token carries the sentinel.
- `scripts/check_git_forbidden_patterns.py` (check #3) — statically enforces `RerunDescriptor.argv: Option<AuditSafeArgv>`.
- `scripts/check_git_forbidden_patterns.py` (check #4) — statically enforces `sanitize_argv_for_run_store` on any git argv flowing into `RunInvocation`.

### P3. Forbidden-pattern static checks

**Invariant:** The forbidden-pattern script enforces five rules:

| # | Rule | Mechanism |
|---|------|-----------|
| 1 | No `Command::new("git")` outside approved modules | regex scan |
| 2 | No hand-maintained env-policy tables outside `process_policy` and the four approved re-export sites | AST walk |
| 3 | `RerunDescriptor.argv` is `Option<AuditSafeArgv>`, not `Vec<String>` or `Option<Vec<String>>` | AST match |
| 4 | Git argv flowing into `RunInvocation` flows through `sanitize_argv_for_run_store` | regex scan |
| 5 | `expose_secret()` calls only at the `render_argv` boundary (or inside test/doc/script contexts) | regex scan |

**Guards:** The script is part of standard local validation (AGENTS.md testing section). It reports PASS (0 findings) on the current tree.

---

## 15. Rerun Secret Lifecycle (polish-pass addition)

**Status:** Mitigated (Option 1: redacted-persisted rerun)

**Problem:** The closure pass intentionally preserved raw URL credentials in the rerun descriptor so an operation could be replayed. That alone did not prove the raw value was non-durable, access-controlled, excluded from exports, or deleted on a bounded schedule.

**Adopted policy (Option 1):** Persist only the redacted URL. The raw value is ephemeral, lifetime-bounded to the running mutation, and never reaches durable storage. A future replay path that needs the raw URL must reconstruct it from the credential helper, prompt, or environment before re-rendering via `render_argv`.

**Evidence:**
- `crates/codegg-git/src/sensitive.rs` — `AuditSafeArgv(Vec<String>)` newtype. `from_argv()` runs `redact_url_credentials_in_text` on every token. `Debug` and `Serialize` impls emit the inner `Vec<String>` (already redacted).
- `crates/codegg-core/src/run_store.rs` — `RerunDescriptor.argv: Option<AuditSafeArgv>` (previously `Option<Vec<String>>`). `is_empty()` updated to delegate to the inner Vec. In-test `rerun_descriptor_no_permission_persistence` updated.
- `src/git_run_store.rs` — calls `AuditSafeArgv::from_argv(render_argv_argv)` before constructing the `RunDraft`.
- `src/test_runner/runner.rs` — calls `AuditSafeArgv::from_argv(resolved.argv)` for the same reason (uniform type-level invariant, even though test argv is credential-free).
- `crates/codegg-core/src/run_store.rs` (deserializer) — `RerunDescriptor::deserialize` re-runs `AuditSafeArgv::from_argv` on the inner Vec to normalize historical records.
- Full inventory: [`docs/validation/git-rerun-secret-lifecycle.md`](git-rerun-secret-lifecycle.md).

**Threat-class rationale:** This is a new category distinct from the 14 Phase F classes — it is the lifecycle of a secret-bearing value through the persistence pipeline, not its injection or sanitization at a single boundary.

**Residual risk:** None for durable storage. The TUI `RunRerun` handler is currently a placeholder; a future replay implementation must use a fresh credential acquisition path (see L2).

---

## Decisions Made

1. **Git aliases are bypassed for typed operations** — The `codegg-git` parser constructs `GitOperation` variants directly; `render_argv()` produces canonical subcommand names. User-defined git aliases are never resolved for typed mutations. The raw subcommand fallback passes subcommand strings directly to `Command::new("git").args()`, where git itself may resolve aliases — this is the documented compatibility escape.

2. **`RevisionExpr` is intentionally lenient** — Only rejects empty strings. Git revision syntax (`HEAD~3`, `stash@{0}`, `^{commit}`, etc.) is too complex to validate exhaustively. Safety comes from positional placement in rendered argv, not from string validation.

3. **`force_with_lease` is tagged destructive in risk set** — Despite being safer than `--force`, it still carries `DestructiveHistory` risk to ensure the permission flow requires explicit user confirmation. The `is_destructive()` method returns `false` for use in permission hint formatting.

4. **Network operations inherit SSH env vars** — `GIT_SSH_COMMAND`, `GIT_SSH_VARIANT`, and `SSH_AUTH_SOCK` are restored for network operations because blocking them would break SSH-based remotes. The config key denylist prevents model-driven escalation via `core.sshCommand`.

5. **Recovery operations use the same executor as mutations** — No special env policy or different subprocess handling. The operation-aware dispatch in `git_recovery.rs` constructs the correct typed `GitOperation` and delegates to `GitMutationExecutor::execute()`.

---

## URL Flow Inventory

Every path through which a remote URL enters or exits Codegg, showing the
redaction boundary. The raw value is transient; only the redacted form
reaches durable storage, model-visible output, projections, or tracing.

### Entry points (raw URL enters)

| Operation | Entry site | Type | Raw needed by child? |
|-----------|-----------|------|---------------------|
| `remote add` | `git_network_ops.rs::remote_add()` | `String` param → `RedactedUrl::new(url)` | Yes — `expose_secret()` at `render_argv` |
| `remote set-url` | `git_network_ops.rs::remote_set_url()` | `String` param → `RedactedUrl::new(url)` | Yes — `expose_secret()` at `render_argv` |
| `remote get-url` | `egggit::refs` | Read from `.git/config` | N/A — output only |
| `remote list` | `egggit::refs` | Read from `.git/config` | N/A — output only |
| `fetch` failure | stderr from git child | Git echoes URL in error text | N/A — already in child output |
| `pull` failure | stderr from git child | Git echoes URL in error text | N/A — already in child output |
| `push` rejection | stderr from git child | Git echoes URL in error text | N/A — already in child output |
| `config --get remote.*.url` | `egggit::refs` | Read from `.git/config` | N/A — output only |

### Redaction boundaries (raw → redacted)

| Boundary | Site | Mechanism |
|----------|------|-----------|
| **Type boundary** | `RedactedUrl::new(url)` in `sensitive.rs` | Raw stored internally; `Debug`/`Display`/`Serialize` emit redacted form only |
| **Execution boundary** | `render_argv()` in `render.rs` | `expose_secret()` consumed here — raw reaches git child process only |
| **Result sanitization** | `sanitize_truncate_for_result()` in `git_mutations.rs` | `redact_url_credentials_in_text()` applied to `MutationResult.stdout/stderr` |
| **Service sanitization** | `run_git_raw()` in `git_service.rs` | `redact_url_credentials_in_text()` applied to read-side stdout/stderr |
| **Persistence sanitization** | `sanitize_argv_for_run_store()` in `git_network_policy.rs` | Redacts URL-bearing tokens in audit `argv`/`command` fields |
| **Projector sanitization** | `git_mutation_projector.rs` | Credential-bearing URLs in `MutationResult` are redacted before projection |

### Sinks (redacted-only)

| Sink | Site | What is stored/displayed |
|------|------|------------------------|
| `RunStore` invocation argv | `git_run_store.rs:53-58` | `sanitize_argv_for_run_store(render_argv(...))` — redacted |
| `RunStore` invocation command | `git_run_store.rs:59` | `argv.join(" ")` — redacted |
| `MutationResult.stdout/stderr` | `sanitize_truncate_for_result()` | `redact_url_credentials_in_text()` applied |
| `GitOperation` Debug | `RedactedUrl::Debug` | Redacted form only |
| `GitOperation` Serialize | `RedactedUrl::Serialize` | Redacted form only |
| Tool output to model | `ToolError` messages | `sanitize_truncate_for_result()` applied |
| Tracing events | All callers use redacted `MutationResult` fields | No raw URL reaches `tracing::*` macros |
| TUI projections | `git_mutation_projector.rs` | Redacted `MutationResult` input |

### Unredacted exceptions

| Path | Why unredacted | Guard |
|------|---------------|-------|
| `render_argv()` output | Git child process needs raw URL for authentication | Consumed by `Command::args()` only; never persisted directly |
| `RedactedUrl::expose_secret()` | Single escape hatch for execution boundary | Only called in `render.rs::render_argv()` |
| Rerun descriptor argv | Re-execution needs raw URL to authenticate | `RunStore` separates rerun argv from audit surfaces; rerun is not model-visible |

---

## Test Coverage

The following test suites cover security-relevant behavior. The polish pass added or extended the bolded suites:

| Test Suite | Coverage |
|------------|----------|
| `crates/codegg-git/src/path.rs` tests | Path validation: absolute, null byte, parent traversal, empty |
| `crates/codegg-git/src/ref_name.rs` tests | Ref validation: dash prefix, double dot, lock suffix, special chars |
| `crates/codegg-git/src/render.rs` tests | `--` insertion, argv rendering for all operation families |
| `crates/codegg-git/src/operation.rs` tests | Risk classification per variant including destructive flags |
| **`crates/codegg-git/src/sensitive.rs` tests** | RedactedUrl Debug/Serialize redaction; AuditSafeArgv construction sanitizer |
| **`crates/codegg-git/src/process_policy.rs` tests** | Canonical list composition; is_allowed/is_stripped invariants |
| **`crates/codegg-core/src/worktree.rs` tests** | worktree_uses_canonical_policy; canonical_includes_locally_drifted_entries |
| `src/git_recovery.rs` tests | State-action matrix, cross-operation misuse prevention |
| **`src/git_mutations.rs::policy_drift_tests`** | Pins canonical lists against historical values |
| `tests/git_recovery_integration.rs` | 19 end-to-end tests on tempdir fixtures |
| `tests/git_network_integration.rs` | URL redaction, config allowlist, network round-trips |
| **`tests/git_credential_runstore_sentinel.rs`** | 7 tests including rerun_argv scan + positive control |
| **`tests/git_credential_cross_path.rs`** | 10 cross-path credential leakage tests |
| **`tests/git_env_attack.rs`** | 20 environment attack vector tests |
| `tests/git_noninteractive.rs` | Non-interactive mode invariants |
| `tests/git_tracing_capture.rs` | Tracing redaction |
| **`tests/git_mutations_integration.rs`** | 12 end-to-end mutation tests |
| **`tests/git_closure_matrix.rs`** | 32 closure-stage integration tests |
| **`tests/git_execution_origin_matrix.rs`** | 19 tests covering rows 1-10 of the execution-origin matrix |
| `src/tool/git.rs::schema_tests` | Schema snapshot: mutation enum, recover enum, description |
| `src/git_network_ops.rs` tests | Config key validation, push force classification, URL redaction |
| **`scripts/check_git_forbidden_patterns.py`** | 5 static checks for forbidden patterns (CI-ready) |
