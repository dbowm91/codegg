# Git Clean Authorization CI C001 — Correct Typed Permission Test Contract

Status: ready for handoff

Baseline: `39f149a15ef529d66787d1d11551872ea22ec384` (`main`, 2026-10-14).

Source: `plans/subsystems/git-clean-authorization-ci-corrective-addendum.md` (C001); historical typed Git work `plans/implementation/linux-execution-isolation/009-m001-typed-git-policy-reconciliation.md` and `plans/closure/linux-execution-isolation/009-status.md`. Accepted ADR-0028; `plans/000-long-term-specification.md` §§4,17,27–29; `plans/003-planning-process.md` §§2,7.
Primary class: **invariant**.

## Objective

Correct the `clean_removes_untracked_files` integration test's expectation that an unauthenticated destructive Git cleanup should succeed. Prove that dry-run preview stays usable; real cleanup without an authorized destructive capability returns the exact typed policy refusal and does not mutate the temporary repo; and a true positive destructive-clean fixture is run **only** if the current trusted execution boundary genuinely supports a test-authorized, confined cleanup. Restore hosted workspace CI progress without weakening Git authorization.

## Evidence and why original verification missed it

- `tests/git_network_integration.rs::executor()` creates `GitMutationExecutor::new().with_env_policy(...).with_timeout(...)` and threads **no** `ExecutionAuthorizationContext`.
- `clean_removes_untracked_files` calls `git_network_ops::clean(&exec,... CleanRequest {force:true,dirs:false,ignored:false,paths:[]})` and demands deletion.
- `crates/egggit/src/process.rs::classify_git_request` returns `RequiresAdditionalAuthorization{DestructiveRepositoryChange}` for real `git clean` and `reset` regardless of `force`; for `clean -n` it explicitly returns `AllowedTrustedNonExecutable`.
- `GitMutationExecutor` offers `with_execution_authorization`, but `run_subprocess` calls `GitEnvPolicy::apply(argv,root)` and does not itself consume authorization to promote `RequiresAdditionalAuthorization` to allowed. Therefore **simply attaching a fabricated authorization snapshot in a test would not fix the policy seam**.
- CI [38606475897](https://github.com/dbowm91/codegg/actions/runs/38606475897) failed `git_network_integration::clean_removes_untracked_files` with `Policy(RequiresAdditionalAuthorization { capabilities: [DestructiveRepositoryChange] })` after 7027 tests had passed. 6614 tests were skipped by fail-fast. The immediately preceding run [38598782570](https://github.com/dbowm91/codegg/actions/runs/38598782570) failed identically.
- Existing `clean_preview_lists_untracked_files` and `clean_broad_ignored_request_is_rejected_by_policy` cover two other behaviors. They do not prove denied real-clean leaves the file untouched.
- Historical Linux C001 correctly introduced typed policy denial, but did not update every older positive Git-clean expectation. Broader local contained Git compatibility remains blocked by separately registered Linux-isolation C002/C003 work.

## Invariants and non-goals

- A destructive Git cleanup requires explicit `DestructiveRepositoryChange` authority, and any additional configured kernel/process/credential constraints; a test's temp directory does not itself grant permission.
- Denial occurs before a child is launched; the untracked fixture remains present, index/HEAD unchanged, and no fallback raw Git or Bash path executes.
- Safe `git clean -n` preview needs no destructive grant and changes nothing. Broad `-x` unscoped cleanup must remain denied by its existing separate policy.
- No blanket `git clean` allowlist, fake executable, unconditional bypass on `#[cfg(test)]`, silent FullHost execution, disabling typed policy checks, injected model approval receipt, or privilege-grant magic based on temp path.
- No storage/protocol migration and no changes to the user's public `git` tool schema. No claim that currently blocked executable local Git or remote Git has become supported.

## Scope and production changes

### A. Fixture normalization (required)

Replace or rename `clean_removes_untracked_files` with a `clean_without_destructive_capability_returns_typed_denial` test or equivalent. Use `fresh_repo()` and a unique untracked TempDir file. Invoke the same public `git_network_ops::clean` API; assert the **typed** `GitMutationError::Policy(GitExecutionPolicyOutcome::RequiresAdditionalAuthorization { capabilities: [DestructiveRepositoryChange] })` rather than a stringified generic error; verify untracked file and tracked commit/HEAD/index remain unchanged, no tombstone/audit success result or Git child was dispatched. If the helper does not expose dispatch count, instrument a fake process adapter or static execution policy check rather than guess from the file alone.

Keep preview positive and immutable, with observed entries; ensure broad ignored cleanup remains explicitly rejected. Add a classifier unit fixture proving `clean -n` is allowed and `clean -f` is permission-gated even in a temp repo.

### B. Conditional authorized positive trajectory (strictly gated)

Inspect `GitMutationExecutor::with_execution_authorization`, `run_subprocess`, `GitEnvPolicy::apply` and any contained process-launcher authority integration. If there is an **existing** trustworthy test-owned way to grant the needed capability and run the command confined to a newly allocated TempDir, add a genuine positive test that leaves only that directory's untracked file removed. Verify accepted signed/scoped receipt, owner identity, containment, no local repo hooks/filter escape and no raw fallback.

If this production route is **not currently implemented** (which is consistent with `run_subprocess` not consuming `execution_authorization`), do **not** make a test-only privilege bypass. Record the positive capability as **blocked on Linux-isolation C002/contained Git** or register a separate production-authority plan after validating architecture. The negative and preview contracts can close this CI corrective independently; do not assert full Git-clean operability.

### C. Regression and report

Update tests' comments to distinguish temporary path containment from actual authorization. Add a static/typed policy assertion to an existing `egggit::process` unit test or Git policy guard so changes to `RequiresAdditionalAuthorization` for real clean are intentional and reviewed. No new CI framework if existing fixtures suffice; avoid duplicated policy tables in shell or tool adapters.

## Work packages

1. **Rebaseline** the exact test, typed error and production execution path; obtain current SHA/host and failing CI log; confirm whether `clean` lacks trusted destructive authority at present.
2. **Negative and preview**: change the stale positive test to verify typed refusal and no mutation; keep dry-run capability-allowed positive. Add no-spawn/count evidence if feasible without production architecture churn.
3. **Honest positive disposition**: inspect trusted authorization injection and native confinement; either add strictly scoped end-to-end positive or explicitly defer to the already registered contained Git roadmap with evidence. Do not weaken policy to resurrect the test.
4. **Qualify** focused Git tests and origin/guard suites; run canonical quick and Clippy; request hosted nextest on final SHA; cross-check failure count and new first-failure if any.
5. **Close** in an independent status record, preserving typed Git C001 historical closure, and update registry/roadmap based on evidence.

## Required tests and commands

```bash
cargo fmt --all -- --check
cargo test -p egggit --lib -- process
cargo test -p codegg --test git_network_integration clean_preview_lists_untracked_files
cargo test -p codegg --test git_network_integration clean_without_destructive_capability_returns_typed_denial
cargo test -p codegg --test git_network_integration
cargo test -p codegg --test git_execution_origin_matrix
cargo test -p codegg --test git_mutations_integration
cargo test -p codegg --test git_env_attack
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The renamed test target is illustrative until implemented; record actual names. After shell C002 removes unsafe host execution tests, obtain hosted CI with full nextest. If this independent Git fix is implemented first, do not run the unchanged full workspace suite containing potentially dangerous BashTool tests; run only safe Git fixtures and quick compile.

## Failure, cancellation, restart and concurrency

Permission refusal is a typed result before process launch and does not mutate Git state; no automatic retry through raw Bash, helper, fallback or scheduler. Success claims require actual scoped capability and verified containment, not merely a process exit of zero. TempDir fixture names unique across nextest parallel workers; Git global/credential environment stripped. Process cancellation/restart must not reinterpret missing authority as approval. No mutable shared global policy for test setup; no real remote, destructive host command or network credential in tests.

## Compatibility and documentation

Update relevant test comments in `tests/git_network_integration.rs` and `architecture/git.md` or `docs/validation/git-security-review.md` only for any genuinely changed behavioral claim. Keep typed `GitExecutionPolicyOutcome` API unchanged. No database or wire changes. Do not rewrite historical `plans/closure/linux-execution-isolation/009-status.md` or claim Linux C002/C003 closed.

## Acceptance criteria

- The no-grant `git clean -f` path returns exactly `RequiresAdditionalAuthorization[DestructiveRepositoryChange]`; no child dispatch/side effect, tracked and untracked files unchanged.
- Dry-run preview remains functional without grant, and broad ignored cleanup is not silently allowed.
- No test-only bypass or privilege promotion; either a verified contained positive clean works with real authority or explicit follow-up remains blocked on existing Linux-isolation plans.
- Focused tests and guards pass, exact-host result recorded; main's hosted test can progress beyond its prior first failure, or the remaining failure is honestly classified.
- Full-suite/strict branch CI evidence is gathered only after shell test hazards are removed; status can be conditional if other independently owned tests remain red.

## Stop conditions

Stop if resolving the test requires unconditional permission bypass, real host-global Git clean, unowned migration to another execution service, fabricated grant, or replacing the typed refusal with a generic spawn failure. Register a separate native authorization/contained-backend plan if no trusted positive route exists.

## Closure evidence

Write `plans/closure/git-clean-authorization-ci-corrective/001-status.md` with exact SHA, before/after fixture matrix, denied/preview tests, conditional positive authority disposition, no-child marker or process instrumentation, applicable Linux sandbox limitations, focused commands/outcomes, hosted CI link, unrelated blockers and final recommendation. Preserve historic Linux and command-authorization closures.
