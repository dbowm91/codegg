# Git Clean Authorization CI — Test Contract Corrective Addendum

Status: closed — C001 closed (implementation `b0392431`; closure `plans/closure/git-clean-authorization-ci-corrective/001-status.md`; Linux contained/remote Git stays separately blocked)

Planning baseline: `39f149a15ef529d66787d1d11551872ea22ec384` (`main`, 2026-10-14).

Canonical: `plans/000-long-term-specification.md` §§4,17,27–29; `plans/003-planning-process.md` §§2,7–10; accepted ADR-0028. Related: `plans/subsystems/linux-execution-isolation-roadmap.md`, `plans/closure/linux-execution-isolation/009-status.md` (typed Git policy C001 closed), `architecture/git.md`, `architecture/security.md`.

## Purpose and observed failure

The Git test suite contains a stale positive-execution assumption independent of the shell test hazard. In `tests/git_network_integration.rs::clean_removes_untracked_files`, the test calls `git_network_ops::clean` using `GitMutationExecutor::new()` with no accepted execution authorization and expects an untracked file to be deleted. The newer `crates/egggit/src/process.rs` execution policy explicitly treats non-dry-run `git clean` (like reset) as `RequiresAdditionalAuthorization { capabilities: [DestructiveRepositoryChange] }`; preview `git clean -n` remains allowed. On [main CI run 38606475897](https://github.com/dbowm91/codegg/actions/runs/38606475897) for `39f149a`, `clean_removes_untracked_files` fails with exactly that typed authorization outcome: 7027 pass, one failure, 6614 skipped by fail-fast. The prior CI run 38598782570 had the same failure. This typed refusal is intended authorization behavior, not evidence that Git should bypass the policy.

## Ownership and invariant

- `egggit::process::classify_git_request` owns the trusted/non-executable vs requires-authorization/containment classification; it must never infer approval from `git clean` argument spelling.
- `GitMutationExecutor`, the daemon tool execution context and canonical approval receipts own user/project/worktree authority; no test may synthesize an unrestricted production grant.
- `git_network_ops::clean_preview` stays safe read-only, while real clean requires an explicit validated destructive capability and any required containment. A policy-denied operation must leave the tempfile marker/file intact, with typed error and zero process dispatch.
- No new broad `git clean` bypass, missing-executable fallback, blanket FullHost default or broad destructive capability grant.
- This is a **test and CI contract correction**, not Linux isolation C002/C003 completion. Existing historically blocked broader Git compatibility remains blocked until native backend qualification.

## Milestone

**C001 — Typed Git-clean authorization test reconciliation** — invariant; **ready**.

Plan: `plans/implementation/git-clean-authorization-ci-corrective/001-typed-git-clean-test-authorization.md`.

No hard new dependencies; accepted typed policy and the failing CI fixture are present. Operational dependency: final hosted Linux CI result, with shell C002 addressed independently if it blocks safe global suite execution.

Acceptance: split preview/no-grant deny into correct regression tests; a positive cleanup path must use only a *real trusted test authorization seam* if the production Git execution owner can actually honor that capability with verified containment. Otherwise explicitly mark positive execution as capability-unavailable and blocked on the already registered Linux containment roadmap, not circumvent the policy. The test suite must stop claiming an unauthenticated destructive Git mutation succeeds. Maintain exact-revision closure in `plans/closure/git-clean-authorization-ci-corrective/001-status.md`.

## Non-goals, migration, qualification

No new Git operation or schema, database migration, tool permit, remote Git transport, symlink traversal relaxation, Linux kernel isolation promotion, or permission redesign. Recheck code and latest CI before implementation; two repos/branches advancing must not create conflicting changes. Verify `tests/git_network_integration.rs`, policy-classifier tests, Git origin matrix, safe local Git operations, guards and hosted CI. If the typed policy is demonstrably wrong relative to accepted ADR or real trusted authorization is already present but dropped, stop and register a focused **production authority plumbing** successor; never silently disable capability checks for this test.
