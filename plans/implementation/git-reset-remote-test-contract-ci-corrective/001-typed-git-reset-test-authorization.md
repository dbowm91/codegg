# Git Reset/Remote Test-Contract C001 — Typed Git-Reset Authorization Fixtures

Status: implemented; closed (implementation `7922bb0e`; closure `plans/closure/git-reset-remote-test-contract-ci-corrective/001-status.md`; hosted `CI / verify` run `38643674863` — scope green, residual recovery red classified as pre-existing Linux-C002 scope)

Baseline: `1cd71e4c` (branch head before implementation).

Source: `plans/subsystems/git-reset-remote-test-contract-ci-corrective-addendum.md` (C001); typed-policy precedent `plans/implementation/linux-execution-isolation/009-m001-typed-git-policy-reconciliation.md` and test-contract precedent `plans/implementation/git-clean-authorization-ci-corrective/001-typed-git-clean-test-authorization.md` (same denied-before-dispatch pattern). Accepted ADR-0028; `plans/000-long-term-specification.md` §§4,17,27–29; `plans/003-planning-process.md` §§2,7.
Primary class: **invariant**.

## 1. Objective

Correct the two stale reset expectations in `tests/git_network_integration.rs` that demand an unauthenticated destructive reset succeed. Prove that `reset --hard` and `reset --soft` without an explicit `DestructiveRepositoryChange` capability return the exact typed policy refusal before any child is launched and leave HEAD, worktree, and index untouched. Restore hosted workspace CI progress past these fixtures without weakening Git authorization.

## 2. Why this milestone is ready

No hard dependencies: the typed reset policy (`classify_git_request` → `RequiresAdditionalAuthorization[DestructiveRepositoryChange]` for every non-dry-run `reset`; reset has no dry-run mode) is closed under Linux C001, and the clean C001 closure proved the `run_subprocess`-does-not-consume-`execution_authorization` seam, so no new interface is needed. The failing fixtures and the accepted policy coexist on the baseline. Operational dependency only: final hosted Linux CI result.

## 3. Current implementation evidence

- `tests/git_network_integration.rs::executor()` builds `GitMutationExecutor::new().with_env_policy(...).with_timeout(...)` with **no** `ExecutionAuthorizationContext`.
- `reset_hard_moves_head_and_drops_changes` calls `git_network_ops::reset_hard(&exec, dir, Some("HEAD~1"))` and asserts `res.success`, HEAD movement, and `extra.txt` deletion. Locally it fails with `Policy(RequiresAdditionalAuthorization { capabilities: [DestructiveRepositoryChange] })`.
- `reset_soft_keeps_working_tree` calls `git_network_ops::reset_soft(&exec, dir, Some("HEAD~1"))` and asserts `res.success` plus staged state. Same typed refusal.
- `GitMutationExecutor::with_execution_authorization` exists, but `run_subprocess` (`src/git_mutations.rs`) clones only `env_policy` and calls `env.apply(argv, root)` → `ensure_trusted_git_request` → `classify_git_request`, which refuses before `cmd.output()`. Attaching a fabricated authorization snapshot in a test cannot promote the outcome.
- `crates/egggit/src/process.rs` has no dedicated reset-mode classifier fixture (fetch is pinned; clean `-n` vs `-f` is pinned by C001). Related risk/validation tests (`git_closure_matrix::reset_hard_classified_as_high_risk`, `command_routing_adversarial::git_reset_hard_fails_validation`) cover risk classes and routing validation, not execution, and are unaffected.

## 4. Invariants that must not regress

- Every non-dry-run `git reset` requires explicit `DestructiveRepositoryChange` authority plus any configured kernel/process constraints; a test TempDir grants nothing.
- Denial precedes child launch: HEAD byte-identical, worktree files present, index unchanged, no success tombstone/audit event on the `Policy` path, no raw Git/Bash fallback.
- No blanket reset allowlist, fake executable, `#[cfg(test)]` bypass, silent FullHost execution, disabled policy checks, injected approval receipt, or temp-path grant magic.
- No claim that contained local Git (Linux C002) has become supported.

## 5. Scope (in / out)

In: replace both stale reset positives with typed-denial fixtures through the same public `git_network_ops::reset_hard` / `reset_soft` API; classifier unit fixture pinning all reset modes; test-comment updates distinguishing TempDir containment from authorization.

Out: no production policy change (`classify_git_request` for `reset` untouched); no positive reset execution (honestly blocked — see §6B); no storage/protocol/tool-schema change; no Linux C002 claim.

## 6. Required production changes

None. Test-only plus a pure classifier unit fixture (no process).

### A. Fixture normalization (required)

- Replace `reset_hard_moves_head_and_drops_changes` with `reset_hard_without_destructive_capability_returns_typed_denial` (or equivalent): `fresh_repo()` + committed `extra.txt`; capture `HEAD` SHA; call the identical public API; assert `expect_err` with **typed** `GitMutationError::Policy(RequiresAdditionalAuthorization { capabilities: [DestructiveRepositoryChange] })`; assert `HEAD` byte-identical after, `extra.txt` still exists, no dispatch side effect.
- Replace `reset_soft_keeps_working_tree` with `reset_soft_without_destructive_capability_returns_typed_denial`: `fresh_repo()` + committed `kept.txt`; capture `HEAD`; assert the same typed refusal; assert `HEAD` identical, `kept.txt` still on disk, `status --porcelain` clean (nothing newly staged — the commit was not moved).
- Keep fixture filenames unique across nextest parallel workers; keep the `GIT_CONFIG_GLOBAL/SYSTEM=/dev/null`, `GIT_TERMINAL_PROMPT=0` hardening in the test helpers.

### B. Honest positive disposition (strictly gated)

Inspect `with_execution_authorization` → `run_subprocess` → `GitEnvPolicy::apply` (already rebaselined in clean C001 §3: authorization is not consumed). Since no trusted production seam exists for a test-authorized confined reset, do **not** fabricate one. Record the positive path as **blocked on Linux-isolation C002/contained Git** in code comments and the closure. The two denial fixtures close this milestone independently; do not assert reset operability.

### C. Regression pin

Add `git_reset_modes_require_destructive_authorization` (or equivalent) in `crates/egggit/src/process.rs` tests: every real reset argv shape (`reset --hard`, `--soft`, `--mixed`, `--merge`, `--keep`, bare `reset`) classifies to `RequiresAdditionalAuthorization[DestructiveRepositoryChange]` and `apply_sync` refuses; changing this classification is a policy change, not a test update.

## 7. Ordered work packages

1. **Rebaseline** the two tests, the typed error, and the `run_subprocess` seam on the baseline SHA; confirm no trusted positive route exists.
2. **Denial fixtures**: replace both positives with typed-denial + invariance assertions.
3. **Classifier pin**: add the reset-mode unit fixture.
4. **Qualify**: focused reset tests + full `git_network_integration` (remaining red must be exactly the three C002 remote fixtures, or zero if C002 lands in the same pass) + origin/guard suites + quick + Clippy; request hosted CI on the final SHA.
5. **Close** in `plans/closure/git-reset-remote-test-contract-ci-corrective/001-status.md` without touching the clean C001 or Linux C001 closures.

## 8. Failure, cancellation, restart, contention semantics

Permission refusal is a typed pre-launch result with no Git state mutation and no automatic retry through raw Bash/helper/fallback/scheduler. Unique TempDir fixtures per test; no mutable shared global policy; no real remote or network credential in tests.

## 9. Compatibility and migration

No database, wire, config, tool-schema, or API change. `GitExecutionPolicyOutcome` unchanged. Do not rewrite historical closures or claim Linux C002/C003 closed.

## 10. Required tests and commands

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

## 11. Documentation updates

Test comments distinguishing TempDir containment from authorization. No `architecture/git.md` behavioral change (the typed `DestructiveRepositoryChange` contract it documents is now tested for reset too).

## 12. Acceptance criteria

- No-grant `reset --hard` / `reset --soft` return exactly `RequiresAdditionalAuthorization[DestructiveRepositoryChange]`; no child dispatch/side effect; HEAD/worktree/index unchanged.
- No test-only bypass or privilege promotion; positive contained reset explicitly deferred to Linux C002.
- Focused tests and guards pass; remaining full-binary red (if any) is honestly classified as the C002 remote scope.

## 13. Stop conditions

Stop if resolving the tests requires unconditional permission bypass, host-global reset, unowned migration to another execution service, fabricated grant, or replacing the typed refusal with a generic spawn failure. Register a production-authority successor instead.

## 14. Closure evidence required

Write `plans/closure/git-reset-remote-test-contract-ci-corrective/001-status.md` with exact SHA, before/after fixture matrix, denial + invariance evidence, positive-path disposition (blocked on Linux C002), focused commands/outcomes, hosted CI link, unrelated blockers, and final recommendation. Preserve historic closures.
