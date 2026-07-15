# Git Polish / Maintainability / Verification Handoff

> Companion to
> [`plans/git_agent_integration_polish_maintainability_verification.md`](../../plans/git_agent_integration_polish_maintainability_verification.md).
>
> This artifact records the post-closure verified state of the Git
> agent integration. It is the canonical reference for future
> contributors; see also `architecture/git.md` for the per-phase
> architectural narrative.

## Status

All polish-pass workstreams are complete as of this commit. No
high- or medium-severity findings remain. Two low-severity items are
accepted and documented below.

## Final crate / module map

```
crates/
├── codegg-git/                         (typed Git model — pure data, no I/O)
│   ├── src/operation.rs                (GitOperation enum, 47 variants)
│   ├── src/parser.rs                   (parse_git_argv → GitOperation)
│   ├── src/render.rs                   (render_argv → Vec<String>; the
│   │                                    ONLY approved expose_secret()
│   │                                    consumer)
│   ├── src/risk.rs                     (GitRiskClass, RiskSet)
│   ├── src/sensitive.rs                (RedactedUrl, AuditSafeArgv,
│   │                                    redact_url_credentials)
│   ├── src/process_policy.rs           (canonical ALLOWED_ENV_VARS /
│   │                                    ALWAYS_STRIPPED_ENV_VARS shared
│   │                                    between root crate and
│   │                                    codegg-core)
│   ├── src/path.rs                     (RepoPath, Pathspec, RepoRoot)
│   ├── src/ref_name.rs                 (BranchName, TagName, RemoteName,
│   │                                    ObjectId, RevisionExpr)
│   ├── src/error.rs                    (ParseError, 9 variants)
│   └── src/origin.rs                   (GitCommandOrigin metadata)
│
├── codegg-core/
│   └── src/worktree.rs                 (create/remove worktree; consumes
│                                        codegg_git::process_policy lists)
│
└── egggit/                             (read-only git facts — `git`
                                         subprocesses here are trusted
                                         read-side and do not need the
                                         hardened env policy)
    └── src/{status,status_v2,log,blame,diff,refs,operation_state,
             conflict,worktree}.rs

src/
├── git_mutations.rs                    (GitEnvPolicy, MutationResult,
│                                        typed mutation framework;
│                                        re-exports canonical env lists)
├── git_network_policy.rs               (NetworkEnvPolicy,
│                                        redact_url_credentials,
│                                        redact_url_credentials_in_text,
│                                        sanitize_argv_for_run_store,
│                                        classify_network_failure)
├── git_network_ops.rs                  (fetch / pull / push / remote /
│                                        config / reset / clean typed
│                                        helpers)
├── git_mutations_ops.rs                (typed mutation helpers — stage,
│                                        commit, branch, stash, merge,
│                                        rebase, cherry-pick, revert,
│                                        restore)
├── git_recovery.rs                     (continue / abort / skip with
│                                        operation-aware guards)
├── git_service.rs                      (GitExecutionService — read
│                                        executor delegating to egggit)
├── git_run_store.rs                    (persist_mutation,
│                                        persist_recovery)
├── git_mutation_projector.rs           (project_mutation,
│                                        project_network_mutation,
│                                        project_destructive_mutation,
│                                        project_recovery)
└── tool/git.rs                         (model-facing git tool:
                                         mutation action, recover,
                                         operation_state, raw subcommand
                                         fallback)

scripts/
├── perf_git_phase_f.sh                 (perf measurement)
└── check_git_forbidden_patterns.py     (E2 static checks)
```

## Execution-origin matrix

The full matrix is asserted by `tests/git_execution_origin_matrix.rs`
(19 tests, all green). Summary:

| # | Origin | Planned backend | Actual backend | Env policy | Redaction boundary | RunStore ownership |
|---|--------|-----------------|----------------|------------|--------------------|--------------------|
| 1 | Native typed read | `Git` | `Git` | `GitEnvPolicy::apply` | (read-only) | n/a |
| 2 | Native typed mutation | `Git` | `Git` | `GitEnvPolicy::apply` | `sanitize_argv_for_run_store` + `redact_url_credentials_in_text` | `DelegatedBackend` |
| 3 | Native raw git subcommand | `Git` | `Git` | `GitEnvPolicy::apply` | `sanitize_argv_for_run_store` | `DelegatedBackend` |
| 4 | Bash simple git read | `Git` (RouteToGit) | `ManagedArgv` | `GitEnvPolicy::apply` | n/a | `Caller` |
| 5 | Bash simple git mutation | `Git` | `RawShell` (gap, see below) | shell policy | shell redaction | `Caller` |
| 6 | Managed git argv fallback | `Git` | `Git` | `GitEnvPolicy::apply` | `sanitize_argv_for_run_store` | `DelegatedBackend` |
| 7 | Raw shell with `\|` / `&&` / `;` | `RawShell` | `RawShell` | (shell policy) | (shell redaction) | `Caller` |
| 8 | TUI git action | `Git` | `Git` | `GitEnvPolicy::apply_sync` | `sanitize_argv_for_run_store` | `DelegatedBackend` |
| 9 | Daemon git action | `Git` | `Git` | `GitEnvPolicy::apply` | `sanitize_argv_for_run_store` | `DelegatedBackend` |
| 10 | Replay / rerun | n/a (placeholder) | n/a | n/a | `AuditSafeArgv` (redacted) | `DelegatedBackend` |

## Permission and risk matrix

| Operation family | Risk classes | Default permission |
|------------------|--------------|--------------------|
| `git status`, `git diff`, `git log`, `git show`, `git blame`, `git branch --list`, `git tag --list`, `git remote --list`, `git worktree list`, `git rev-parse`, `git for-each-ref` | `ReadOnly` | `Allow` |
| `git add`, `git restore --staged` | `IndexMutation` | `Allow` (Bash routing) / `Allow` (tool) |
| `git commit`, `git switch`, `git restore` (worktree) | `WorktreeMutation` | `Ask` |
| `git branch <name>`, `git tag <name>`, `git remote add`, `git remote remove`, `git remote set-url`, `git config`, `git stash push/apply/pop/drop` | `RefMutation`, `RepositoryConfigMutation` | `Ask` |
| `git fetch`, `git pull` | `NetworkRead` | `Ask` |
| `git push` (normal, `--force-with-lease`) | `NetworkWrite` | `Ask` |
| `git push --force`, `git reset --hard`, `git clean -f` | `NetworkWrite` + `DestructiveHistory` or `DestructiveWorktree` | `Deny` |
| `git merge`, `git rebase`, `git cherry-pick`, `git revert`, `git stash` | `HistoryIntegration` | `Ask` |
| `git reset` (default / soft / mixed / merge / keep) | `IndexMutation` / `WorktreeMutation` / `HistoryIntegration` | `Ask` (tool path; `--hard` is `Deny`) |

## Environment policy ownership

Single source of truth: `codegg_git::process_policy`.

| Consumer | Source | Re-export? |
|----------|--------|------------|
| `src/git_mutations.rs::GitEnvPolicy::apply` | `codegg_git::process_policy` | `pub use` of `ALLOWED_ENV_VARS` / `ALWAYS_STRIPPED_ENV_VARS` for back-compat with downstream callers / docs |
| `src/git_mutations.rs::GitEnvPolicy::apply_sync` | same | same |
| `src/git_network_ops.rs::NetworkEnvPolicy::apply_to_command` | baseline + `NETWORK_ALLOWED_ENV_VARS` (root-only) | n/a |
| `crates/codegg-core/src/worktree.rs::hardened_git_command` | `codegg_git::process_policy` | `pub use` aliases |

Drift guards:

- `src/git_mutations.rs::policy_drift_tests` pins the canonical list
  entries against the historical Phase F values.
- `codegg-core/src/worktree::tests::worktree_uses_canonical_policy`
  pins that the core worktree helper consumes the canonical lists.
- `scripts/check_git_forbidden_patterns.py` reports any
  hand-maintained env-policy table outside the four approved paths.

## Secret lifecycle decision

Adopted: **Option 1 (redacted-persisted rerun)** from the polish
plan. The `RerunDescriptor.argv` field is now
`Option<AuditSafeArgv>` (where `AuditSafeArgv` is a newtype in
`codegg_git::sensitive`). The only construction path
(`AuditSafeArgv::from_argv`) runs the URL sanitizer on every token.
The deserializer also re-runs the sanitizer on load to normalize
historical records.

The raw URL still flows through `MutationResult.operation` for the
duration of the mutation; it reaches git's argv via
`render_argv` (the only approved `expose_secret` consumer). After
the mutation completes, the raw value is dropped — durable storage
never sees it.

A future replay path that needs the raw URL must reconstruct it
from the user (credential helper, prompt, or env) before
re-rendering via `render_argv`. The TUI `RunRerun` handler is
currently a placeholder (`src/tui/app/mod.rs:3615`); it does not
attempt to re-execute.

Full inventory and rationale: [`docs/validation/git-rerun-secret-lifecycle.md`](../../docs/validation/git-rerun-secret-lifecycle.md).

## Durable vs ephemeral RunStore fields

| Field | Type | Durable? | Credential-bearing? | Redaction applied |
|-------|------|----------|---------------------|-------------------|
| `RunManifest.invocation.command` | `String` | yes | no (audit) | `sanitize_argv_for_run_store` |
| `RunManifest.invocation.argv` | `Option<Vec<String>>` | yes | no (audit) | `sanitize_argv_for_run_store` |
| `RunManifest.rerun.argv` | `Option<AuditSafeArgv>` | yes | no (redacted) | type-level invariant |
| `RunManifest.rerun.script_source_ref` | `Option<String>` | yes | no | n/a |
| `RunManifest.rerun.cwd` / `workspace_root` | `PathBuf` | yes | no | n/a |
| `RunManifest.artifacts[*]` | bytes | yes | no | `redact_url_credentials_in_text` (for stdout/stderr/projection) |
| `RunManifest.projection` | bytes | yes | no | redaction via projector pipeline |
| `RunManifest.sandbox` | record | yes | no | n/a |
| `RunManifest.changes` | paths | yes | no | n/a |
| `RunManifest.planned_backend` | enum | yes | no | n/a |
| `RunManifest.actual_backend` | enum | yes | no | n/a |
| `RunManifest.fallback` | record | yes | no | n/a |
| `RunManifest.ownership` | enum | yes | no | n/a |
| `IndexEntry.command` | `String` | yes (JSONL index) | no | `sanitize_argv_for_run_store` |
| `RunSummary.command` | `String` | yes (in-memory only) | no | inherits from index |

## Supported and fallback operation matrix

| Operation family | Support | Notes |
|------------------|---------|-------|
| Local mutations (add, commit, branch, switch, restore, reset, stash) | typed | All Phase D operations |
| Network (fetch, pull, push, remote *) | typed + network policy | Phase E |
| Configuration (config get/set/unset) | typed + allowlist | Phase E |
| Destructive (reset --hard, reset --merge, reset --keep, clean) | typed + destructive policy | Phase E |
| Recovery (continue, abort, skip) | typed + operation-aware | Phase F |
| Conflicts | typed model, no auto-resolve | Phase F |
| Operation state probe (typed) | read-only | Phase F |
| `raw subcommand` tool fallback | `Command::new("git")` via `GitEnvPolicy::apply` | Phase F closure |
| `ManagedGitArgv` parser fallback | consumed via `render_argv` | Phase B |
| `RawShellRequired` | shell-owned execution | Phase B |

## Validation commands and results

### Forbidden-pattern static check

```bash
python3 scripts/check_git_forbidden_patterns.py
```

Result: `PASS (0 findings)`.

### Git-focused test suite (141 tests across 9 binaries)

```bash
cargo test --test git_credential_runstore_sentinel \
           --test git_credential_cross_path \
           --test git_env_attack \
           --test git_noninteractive \
           --test git_tracing_capture \
           --test git_network_integration \
           --test git_mutations_integration \
           --test git_recovery_integration \
           --test git_closure_matrix
```

Result: `141 passed`.

### Execution-origin matrix (19 tests)

```bash
cargo test --test git_execution_origin_matrix
```

Result: `19 passed`.

### Drift guards

```bash
cargo test -p codegg-git   # 354 + 7 ignored (covers process_policy + sensitive)
cargo test -p codegg-core  # 119 (covers worktree policy drift tests)
```

Result: all green.

### Performance measurement

```bash
bash scripts/perf_git_phase_f.sh
```

Result (1000-file repo, 5 iterations):

| Metric | avg | p50 | p99 |
|--------|-----|-----|-----|
| `rich_repo_status` | 77 ms | 73 ms | 96 ms |
| `detect_operation_state` | 51 ms | 52 ms | 52 ms |
| `project_recovery` | 164 ms | 163 ms | 168 ms |
| `sidebar_refresh` | 98 ms | 96 ms | 108 ms |
| `runstore_persist` | 82 ms | 83 ms | 84 ms |
| `diff_stat_1000files` | 72 ms | 72 ms | 77 ms |

Sidebar worst-case 108 ms — well under the 3000 ms timeout. No
regressions vs. Phase F baseline.

## Cross-platform status

See [`docs/validation/git-cross-platform.md`](../../docs/validation/git-cross-platform.md)
for the full matrix. The polish pass did not change any
cross-platform behavior:

- `HOME` / `USERPROFILE` / `HOMEDRIVE` / `HOMEPATH` — preserved via
  `HOME` in `ALLOWED_ENV_VARS`. Windows-specific vars would need a
  `#[cfg(windows)]` overlay; deferred (no Windows CI today).
- `PATH` — preserved.
- `TMPDIR` / `TMP` / `TEMP` — only `TMPDIR` is in the allowlist; on
  Windows, git respects `%TMP%` natively through the env. Defer
  expansion.
- SSH agent — `SSH_AUTH_SOCK` / `SSH_AGENT_PID` preserved.
- Non-UTF-8 paths — still use byte checks (`RepoPath` / `Pathspec`);
  no regression.

## Known limitations

### Low-severity

1. **Bash simple git mutation (matrix row 5) routes through
   raw shell.** `intent_kind_to_family(GitMutating)` returns `None`
   in `src/tool/bash.rs`, so when active routing is enabled a
   `git commit -m foo` command is classified as `GitMutating`
   but dispatched as `RawShell`. The classifier is correct; the
   routing gate is intentionally conservative for mutations because
   the model-facing `git` tool already exposes typed mutations. A
   future `GitMutate` family could close the gap, but doing so is
   out of scope for the polish pass (no behavior change without a
   tracked ask).
2. **TUI `RunRerun` is a placeholder** (`src/tui/app/mod.rs:3615`).
   The handler emits `TuiCommand::ShellRerun { id: 0 }` and never
   reads back `rerun.argv`. The polish pass strengthened the
   invariant (redacted argv), so a future replay implementation
   must reconstruct the raw URL — see
   [`docs/validation/git-rerun-secret-lifecycle.md`](../../docs/validation/git-rerun-secret-lifecycle.md).
3. **Windows env-var overlays** (`USERPROFILE`, `HOMEDRIVE`,
   `HOMEPATH`, `PATHEXT`) are not in the canonical list. Defer
   until Windows CI is added.

### Deferred work

- Parser / renderer family splits (Workstream C2) — the files are
  large but the ownership boundaries are already well-isolated by
  module (`operation.rs` already contains the variants,
  `parser.rs` handles parsing, `render.rs` handles rendering).
  Splitting into `parser/{read,staging,…}` would add churn without
  reducing review cost. Defer.
- Git tool dispatch split (Workstream C3) — `src/tool/git.rs` is
  1127 lines but is already organized by dispatch path
  (`dispatch_operation_state`, `dispatch_recover`, schema
  definitions). The schema snapshot tests pin the public surface.
  Defer.
- Property / fuzz tests (Workstream E3) — the existing
  cross-crate redaction test, sentinel suite, and parser
  round-trip tests cover the high-value cases. Adding a
  `proptest` harness is straightforward but adds CI time without
  materially closing risk on top of the closure pass.

## Closure commit references

| Commit | Theme |
|--------|-------|
| `cb192e9` | Phase F corrective security closure — `RedactedUrl` + hardened env policy |
| `c2e806f` | Corrective closure completion — adversarial credential, RunStore, tracing, environment verification |
| `53b2beb` | URL-flow inventory, B4 render_argv boundary test, clippy fix |
| `86c16a9` | Polish plan proposal |
| *this commit* | Polish / maintainability / verification handoff |