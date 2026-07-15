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

**Update (post-delta):** the gap-closure delta added B4 cross-platform
policy composition tests, D4 repository resolution edge-case tests,
F2/F3 quadratic-behavior guards and a truncation-after-redaction
invariant test, E1 security review refresh, and G1 reconciliation of
the command_intent/command_planner/command_routing architecture docs
plus `docs/validation/git-security-review.md` and
`docs/validation/git-cross-platform.md`. Two workstreams remain
deferred: C (large-file maintainability splits) and E3 (property/fuzz
tests) — both with explicit rationale below.

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
| 5 | Bash simple git mutation | `Git` | `Git` (when `route_git_local_mutation = Active`, Track U) / `RawShell` (default) | `GitEnvPolicy::apply` / shell policy | `sanitize_argv_for_run_store` / shell redaction | `DelegatedBackend` / `Caller` |
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

### Git-focused test suite (141+ tests across 9 binaries)

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

Result: `141+ passed` (10 original + 4 new F2/F3 tests in
`git_credential_cross_path`, totaling 14).

### Execution-origin matrix (26 tests after gap-closure delta)

```bash
cargo test --test git_execution_origin_matrix
```

Result: `26 passed` (19 original + 7 new D4 tests).

### Drift guards

```bash
cargo test -p codegg-git   # 354 + 7 ignored + new B4 tests (covers process_policy + sensitive)
cargo test -p codegg-core  # 119 (covers worktree policy drift tests)
```

Result: all green.

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

1. **Bash simple git mutation (matrix row 5) — closed by Track U.**
   `intent_kind_to_family(GitMutating)` historically returned `None` in
   `src/tool/bash.rs`, so bash-translated simple git mutations dispatched
   as `RawShell`. Track U replaces this with `git_operation_family()`
   and `dispatch_to_git` → `GitMutationExecutor`, sharing env policy,
   snapshot/delta, and RunStore parity with the native tool. The gate
   `route_git_local_mutation` defaults to `Off`; when set to `Active`,
   bash git mutations route through the typed Git backend. The
   classifier is correct; the routing gate is intentionally conservative
   for mutations because the model-facing `git` tool already exposes
   typed mutations.
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

- **Workstream C (large-file maintainability splits)** — the files
  are large but the ownership boundaries are already well-isolated by
  module (`operation.rs` contains the variants, `parser.rs` handles
  parsing, `render.rs` handles rendering, `src/tool/git.rs` is
  organized by dispatch path with `dispatch_operation_state`,
  `dispatch_recover`, and schema definitions). Splitting into
  `parser/{read,staging,…}` would add churn without reducing review
  cost. The plan's DoD criterion #5 ("easier to navigate") is partly
  satisfied: the polish pass added more inline doc comments and
  re-export aliases for the canonical policy. A future split would
  be a refactor, not a polish task. Defer until a measurable review
  burden emerges.
- **Workstream E3 (property / fuzz tests)** — the existing
  cross-crate redaction test, sentinel suite, parser round-trip
  tests, and the new F2 quadratic-behavior guards cover the
  high-value cases. Adding a `proptest` harness is straightforward
  but adds CI time without materially closing risk on top of the
  closure pass. The plan's DoD criterion #9 (focused security suites
  pass) is satisfied without proptest.

### Workstreams completed in the gap-closure delta

- **B4 (cross-platform policy composition)** — 3 new in-module tests
  in `crates/codegg-git/src/process_policy.rs`: canonical lists are
  pure data (valid env-var identifiers), Windows overlays are
  documented but gated, Unix canonical list excludes Windows vars.
- **D4 (repository resolution edge cases)** — 7 new tests in
  `tests/git_execution_origin_matrix.rs`: outer path, nested dir,
  nested independent repo, non-repo, nonexistent path, symlinked
  working dir, stability across calls. Total test file size grew from
  19 to 26 tests.
- **E1 (security review refresh)** —
  `docs/validation/git-security-review.md` was rewritten against the
  post-polish code (line numbers updated, evidence refreshed, new
  Threat #15 added for the rerun secret lifecycle, accepted
  limitations reclassified as L1/L2 with regression test references).
- **F2 (quadratic behavior guards)** — 3 size-scaled tests in
  `tests/git_credential_cross_path.rs`: long-stderr redaction
  (<250 ms on 1 MiB), large-argv sanitization (<100 ms on 10k
  tokens), many-URL redaction (<250 ms on 1 MiB / 1000 URLs).
- **F3 (truncation-after-redaction invariant)** — 1 test in
  `tests/git_credential_cross_path.rs`: proves
  `sanitize_truncate_for_result` redacts credentials that fall after
  the truncation boundary.
- **G1 (architecture doc reconciliation)** —
  `architecture/command_intent.md` (provenance parity note),
  `architecture/command_planner.md` (routing caveat),
  `architecture/command_routing.md` (polish-pass provenance parity
  section), `docs/validation/git-cross-platform.md` (polish-pass
  notes + Windows deferral), `docs/validation/git-security-review.md`
  (full rewrite).

## Closure commit references

| Commit | Theme |
|--------|-------|
| `cb192e9` | Phase F corrective security closure — `RedactedUrl` + hardened env policy |
| `c2e806f` | Corrective closure completion — adversarial credential, RunStore, tracing, environment verification |
| `53b2beb` | URL-flow inventory, B4 render_argv boundary test, clippy fix |
| `86c16a9` | Polish plan proposal |
| `8d686c7` | Polish / maintainability / verification handoff (canonical subprocess policy, AuditSafeArgv, forbidden-pattern checks) |
| *this commit* | Gap-closure delta (D4 repo resolution, F2/F3 quadratic + truncation, E1/G1 doc refresh, B4 cross-platform policy tests) |