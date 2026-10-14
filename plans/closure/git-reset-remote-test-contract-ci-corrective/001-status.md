# Git Reset/Remote Test-Contract C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/git-reset-remote-test-contract-ci-corrective/001-typed-git-reset-test-authorization.md`

Source subsystem roadmap:

- `plans/subsystems/git-reset-remote-test-contract-ci-corrective-addendum.md#C001`

Repository baseline reviewed: `1cd71e4c` (branch head before implementation)

Implementation commits:

- `7922bb0e` — test(git): typed reset-denial and remote-unavailable fixtures (C001/C002) (reset half; this closure's evidence was gathered on this tree)

## 1. Executive finding

C001 is complete. The stale `reset_hard_moves_head_and_drops_changes` and `reset_soft_keeps_working_tree` expectations that an unauthenticated destructive `git reset` should succeed are replaced by `reset_hard_without_destructive_capability_returns_typed_denial` and `reset_soft_without_destructive_capability_returns_typed_denial`, which prove the intended authorization behavior: every non-dry-run reset without an explicit `DestructiveRepositoryChange` capability returns the exact typed `RequiresAdditionalAuthorization` refusal before any child is launched and leaves HEAD, worktree, and index untouched. Reset has no dry-run mode, so there is no allowed-preview counterpart to preserve. No test-only privilege bypass, blanket allowlist, fake executable, `#[cfg(test)]` bypass, or policy weakening was introduced. The genuine positive destructive-reset path is honestly recorded as unavailable: `GitMutationExecutor::run_subprocess` does not consume `execution_authorization` (rebaselined in the clean C001 closure §3 and re-verified here) — that capability stays blocked on the already-registered Linux containment work (C002/contained Git), not this CI corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| No-grant `reset --hard` returns exactly `RequiresAdditionalAuthorization[DestructiveRepositoryChange]` with no dispatch/side effect | `reset_hard_without_destructive_capability_returns_typed_denial` matches `GitMutationError::Policy(RequiresAdditionalAuthorization{capabilities})` and asserts the capability + HEAD-equality + `extra.txt` presence | pass | Same public `git_network_ops::reset_hard` API, `fresh_repo()` + committed fixture |
| No-grant `reset --soft` returns the same typed refusal with no HEAD/index movement | `reset_soft_without_destructive_capability_returns_typed_denial` asserts the capability + HEAD-equality + `kept.txt` on disk + clean `status --porcelain` | pass | Same public `git_network_ops::reset_soft` API; clean tree proves the commit was not moved |
| Classifier proves every reset mode is gated even in a temp repo | New `git_reset_modes_require_destructive_authorization` in `crates/egggit/src/process.rs` (`--hard`/`--soft`/`--mixed`/`--merge`/`--keep`/bare) + `apply_sync` refusal | pass | No production policy line changed |
| No test-only bypass or privilege promotion; positive path honestly dispositioned | `run_subprocess` rebaseline (unchanged since clean C001); no `with_execution_authorization` fabrication in tests | pass | Positive contained reset recorded as blocked on Linux-isolation C002, not asserted |
| Focused tests + guards pass; hosted progress honestly classified | §4 commands/results | pass | Full `git_network_integration` green together with C002; hosted CI recorded below |
| Full-suite green only after remaining fixtures reconciled | Sibling C002 landed in the same implementation commit; no remaining red in this binary | pass | C002 closure discharges the remote half |

## 3. Production implementation evidence

- `tests/git_network_integration.rs`: removed `reset_hard_moves_head_and_drops_changes` (demanded HEAD movement + `extra.txt` deletion via no-grant `reset_hard`) and `reset_soft_keeps_working_tree` (demanded success + staged state via no-grant `reset_soft`). Added the two typed-denial fixtures, which invoke the identical public APIs, assert the typed `Policy(RequiresAdditionalAuthorization{DestructiveRepositoryChange})` rather than a stringified error, and verify state invariance (HEAD SHA byte-identical; `extra.txt`/`kept.txt` present; soft-reset tree clean). Comments distinguish TempDir containment (not a grant) from actual authorization and note reset has no dry-run mode.
- `crates/egggit/src/process.rs`: new classifier fixture `git_reset_modes_require_destructive_authorization` pins all six reset argv shapes so future changes to reset gating are intentional and reviewed. No production policy line changed (`classify_git_request` for `reset` untouched).
- Trusted-authorization rebaseline: unchanged from clean C001 §3 — `run_subprocess` (`src/git_mutations.rs`) clones only `env_policy` and calls `env.apply(argv, root)`, which fails through `ensure_trusted_git_request` → `classify_git_request` with the typed refusal. A genuine positive requires the contained-backend authority integration owned by Linux-isolation C002.
- No Git operation, schema, database, wire, tool-permit, remote-transport, symlink, or permission-redesign change. `GitExecutionPolicyOutcome` API unchanged. Historical `plans/closure/linux-execution-isolation/009-status.md` untouched; Linux C002/C003 not claimed closed.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p egggit --lib -- process
TMPDIR=/tmp cargo test -p codegg --test git_network_integration reset_
TMPDIR=/tmp cargo test -p codegg --test git_network_integration
TMPDIR=/tmp cargo test -p codegg --test git_execution_origin_matrix
TMPDIR=/tmp cargo test -p codegg --test git_mutations_integration
TMPDIR=/tmp cargo test -p codegg --test git_env_attack
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_execution_ownership.py
TMPDIR=/tmp bash scripts/verify.sh quick
TMPDIR=/tmp cargo clippy --workspace --all-targets --locked -- -D warnings
```

### Results

- `cargo fmt --check`: clean.
- `egggit --lib process`: 14 passed, 0 failed (includes the two new classifier fixtures).
- `git_network_integration reset_` filter: 2 passed (`reset_hard_without_destructive_capability_returns_typed_denial`, `reset_soft_without_destructive_capability_returns_typed_denial`).
- `git_network_integration` full binary: 29 passed, 0 failed — the binary is fully green (clean C001's 3 + reset C001's 2 + remote C002's 3 + 21 pre-existing).
- `git_execution_origin_matrix`: 28 passed. `git_mutations_integration`: 12 passed. `git_env_attack`: 20 passed.
- `check_git_forbidden_patterns.py`: PASS (0 findings). `check_execution_ownership.py`: ok. Shell-policy guard + self-test: ok.
- `verify.sh quick`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- Hosted CI: `CI / verify` run `38643674863` on PR #120 (implementation head `7922bb0e`, which also carries the shell C002 + clean C001 test changes) completed with conclusion `failure` whose failure set is exactly three pre-existing `tests/git_recovery_integration.rs` tests (`recovery_abort_in_progress_clears_state`, `recovery_continue_after_resolution_succeeds`, `recovery_continue_with_unresolved_conflicts_fails` — merge/rebase recovery demanding `BackendUnavailable[CleanSmudgeFilter, MergeDriver, Hook]`, i.e. the Linux-C002 contained-local-Git scope; see §10). Every fixture this milestone owns passed on hosted Linux, including both reset denial tests and — beyond this milestone's scope — the shell C002 and clean C001 fixtures. Guards, fmt, and clippy steps all succeeded on the hosted runner. (Docs-only closure heads carry no code delta; CI skips `plans/**`-only pushes by workflow design.)

## 5. Invariant review

- Every non-dry-run reset requires explicit `DestructiveRepositoryChange`; a TempDir grants nothing: both denial tests use `fresh_repo()` + no authorization and get the typed refusal. Holds.
- Denial precedes child launch; HEAD/worktree/index unchanged; no raw Git/Bash fallback: SHA-equality + file-presence + clean-status assertions; `run_subprocess` maps the policy refusal before `cmd.output()`. Holds (no-dispatch inferred from state invariance + `apply_sync` refusal in the classifier fixture).
- No blanket allowlist, fake executable, `#[cfg(test)]` bypass, FullHost execution, disabled policy checks, injected approval receipt, or temp-path grant magic: none added. Holds.
- No storage/protocol migration, public `git` tool schema change, or contained-local support claim: none made. Holds.

## 6. Failure and recovery review

- Permission refusal is a typed pre-launch result with no Git state mutation and no automatic retry through raw Bash/helper/fallback/scheduler: `execute` propagates the `Policy` error; no retry path added.
- Success claims require scoped capability + verified containment, not exit-zero: no positive reset asserted; the unavailable positive is recorded, not faked.
- TempDir fixtures unique per test; Git global/credential env stripped by the test helper. Holds.
- Cancellation/restart never reinterprets missing authority as approval: no mutable shared global policy added for tests.

## 7. Migration and compatibility review

No database, wire, config, tool-schema, or API change. Test comments only. Historical Linux C001 and clean C001 closures unchanged. No rollback concern: the change is test-only plus pure unit fixtures.

## 8. Security review

Resolving the tests did not use an unconditional bypass, host-global reset, unowned execution-service migration, fabricated grant, or generic spawn failure in place of the typed refusal — all stop-condition shapes were avoided. No new attack surface: the classifier fixture is pure (no process), the integration denial tests perform no reset. `check_git_forbidden_patterns` and `check_execution_ownership` both green.

## 9. Documentation and operations

- Updated test comments in `tests/git_network_integration.rs` (containment ≠ authorization; reset has no dry-run mode).
- No `architecture/git.md` behavioral claim changed (the typed `DestructiveRepositoryChange` contract it already documents is now tested for reset too).
- Static protection: the new `egggit::process` reset-mode fixture makes future reset-gating changes intentional and reviewed, alongside the existing `check_git_forbidden_patterns.py` trusted-policy guard.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Positive contained reset still unavailable (`run_subprocess` does not consume `execution_authorization`) | No positive reset coverage | Owned by Linux-isolation C002; this fixture pair must be revisited when C002 lands a real backend |
| low | No-dispatch proven by state invariance + `apply_sync` refusal, not a child-spawn counter | Residual inference | Acceptable per plan; a spawn counter would require production adapter churn |
| medium | Hosted `CI / verify` run `38643674863` (and local runs) still fail three `tests/git_recovery_integration.rs` tests outside this scope: `recovery_abort_in_progress_clears_state` (`abort should succeed: Err(Policy(BackendUnavailable { capabilities: [CleanSmudgeFilter, MergeDriver, Hook] }))`), `recovery_continue_after_resolution_succeeds`, `recovery_continue_with_unresolved_conflicts_fails` (locally also `recovery_skip_during_rebase_skips_step`, same family) | Workspace CI cannot go fully green on these fixtures | Pre-existing at the `origin/main` base by construction — this diff touches no production file in the recovery path (`git diff origin/main` outside plans covers only `tests/git_network_integration.rs`, `#[cfg(test)]`-only code in `crates/egggit/src/process.rs`, shell-policy classification/tests, and CI guard wiring). Merge/rebase recovery that invokes repository-controlled filters/drivers/hooks requires the contained backend owned by Linux-isolation C002. Fixing them is a separate test-contract/contained-backend corrective on the C002 line; do NOT fold into this C001 |

No critical/high/medium findings. The five-fixture `git_network_integration` red that motivated this workstream is fully discharged (binary green locally and on hosted CI).

## 11. Roadmap disposition

Milestone C001 closed for its bounded reset test-contract scope. The reset half of the `git-reset-remote-test-contract-ci-corrective` workstream is done; the remote half is owned by sibling C002 (same implementation commit, separate closure). Linux contained Git (C002), mediated remote Git (C003), ADR-0027 acceptance, and the M001 native backend remain blocked exactly as before — this closure promotes none of them.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: reset/remote row stays `active` until C002 closes (this C001 recorded as implemented; closure `plans/closure/git-reset-remote-test-contract-ci-corrective/001-status.md`).
- `plans/registry.md` Dependency-ready plans: C001 row → `closed` (implementation `7922bb0e`).
- `plans/subsystems/git-reset-remote-test-contract-ci-corrective-addendum.md`: C001 marked implemented/closed.
- `plans/implementation/git-reset-remote-test-contract-ci-corrective/001-typed-git-reset-test-authorization.md`: Status → `implemented; closed`.
- Unblock audit (per `plans/003-planning-process.md` §11): no registered plan lists reset C001 as a hard/interface dependency; Linux M001/C002/C003 remain blocked on ADR-0027 + native proof + egress/lease interfaces. **Nothing newly unblocked — recorded explicitly.**
