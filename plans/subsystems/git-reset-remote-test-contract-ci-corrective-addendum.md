# Git Reset/Remote Test-Contract CI — Corrective Addendum

Status: active — C001/C002 ready (implementation on the shell/Git-clean corrective branch)

Planning baseline: `1cd71e4c` (branch `planning/shell-policy-adr0028-and-git-clean-ci-correctives`, 2026-10-14).

Canonical: `plans/000-long-term-specification.md` §§4,17,27–29; `plans/003-planning-process.md` §§2,7–10; accepted ADR-0028. Related: `plans/subsystems/linux-execution-isolation-roadmap.md` (C002 contained local Git, C003 authorized remote Git), `plans/closure/linux-execution-isolation/009-status.md` (typed Git policy C001 closed), `plans/subsystems/git-clean-authorization-ci-corrective-addendum.md` (C001 closed, same test-contract pattern), `architecture/git.md`, `architecture/security.md`.

## Purpose and observed failure

After Git-clean C001 (`b0392431`) replaced the stale `clean_removes_untracked_files` positive with a typed-denial fixture, `tests/git_network_integration.rs` still contains five stale positive-execution expectations that demand unauthenticated or uncontained execution. Locally the full binary runs 24 passed / 5 failed; on hosted main the same fixtures fail once the suite reaches them. The five failures split into two ownership lines:

1. **Reset (2 tests):** `reset_hard_moves_head_and_drops_changes` and `reset_soft_keeps_working_tree` call `git_network_ops::reset_hard` / `reset_soft` with `GitMutationExecutor::new()` minus authorization and demand HEAD movement. `crates/egggit/src/process.rs::classify_git_request` returns `RequiresAdditionalAuthorization { capabilities: [DestructiveRepositoryChange] }` for every non-dry-run `reset` (there is no dry-run reset mode). The typed refusal is intended authorization behavior, not evidence that reset should bypass the policy.
2. **Remote (3 tests):** `fetch_from_local_bare_remote_succeeds`, `push_to_local_bare_remote_succeeds`, and `pull_from_local_bare_remote_succeeds` call `git_network_ops::fetch` / `push` / `pull` against a local bare remote and demand success. `classify_git_request` returns `RequiresContainedExecution { capabilities: [RemoteHelper, Network, Credential] }` for `fetch`/`pull`/`push`/`clone` regardless of URL scheme (including `file://` local paths), which `ensure_trusted_git_request` surfaces as typed `BackendUnavailable`. No contained remote backend exists on this branch: Linux C003 (authorized remote Git) is blocked on C001 + M005/M006 + transport policy, and file-remote qualification is explicitly future C003 work ("File/local remotes can be qualified independently of networked transports" — not yet qualified). A local path remote is not an automatic safety grant: repository-controlled remote helpers, URL rewrites, and credential helpers still apply.

## Ownership and invariant

- `egggit::process::classify_git_request` owns the reset/remote classification; tests must never infer approval from argv spelling or from a `file://` remote.
- `GitMutationExecutor`, the daemon tool execution context, and canonical approval receipts own authority; no test may synthesize an unrestricted production grant or a fake contained backend.
- `run_subprocess` does not consume `execution_authorization` to promote either refusal (proven in the clean C001 closure §3), so attaching a fabricated authorization snapshot in a test would not promote the outcome — correctly, per the stop condition.
- An unavailable capability is a first-class typed outcome (`RequiresAdditionalAuthorization` / `BackendUnavailable`), never a fabricated process error and never a passing success. This corrective asserts the typed outcomes and records the positive paths as blocked on the already-registered Linux containment line — it does **not** close Linux C002/C003, qualify file remotes, or loosen the classifier.
- This is a **test and CI contract correction**, not containment completion. Historical Linux M001/C001 closures stay untouched.

## Milestones

**C001 — Typed Git-reset authorization test reconciliation** — invariant; **ready**.

Plan: `plans/implementation/git-reset-remote-test-contract-ci-corrective/001-typed-git-reset-test-authorization.md`.

No hard new dependencies; accepted typed policy and the failing fixtures are present. Operational dependency: final hosted Linux CI result.

Acceptance: both stale reset positives become typed-denial fixtures asserting exactly `RequiresAdditionalAuthorization[DestructiveRepositoryChange]` with HEAD/worktree/index invariance and no dispatch; a classifier unit fixture pins every reset mode's gating. The suite stops claiming an unauthenticated destructive reset succeeds. Closure in `plans/closure/git-reset-remote-test-contract-ci-corrective/001-status.md`.

**C002 — Remote Git containment test reconciliation** — invariant; **ready** (interface: Linux C003 owns the positive path).

Plan: `plans/implementation/git-reset-remote-test-contract-ci-corrective/002-remote-git-unavailable-test-reconciliation.md`.

No hard new dependencies; the typed `BackendUnavailable` contract and local bare-remote fixtures are present. Interface dependency: Linux C003 (`011-m001-authorized-remote-git-recovery.md`) owns any future positive remote execution — this C002 consumes its blocked status, never its capability.

Acceptance: all three stale remote positives become typed-unavailable fixtures asserting exactly `BackendUnavailable[RemoteHelper, Network, Credential]` with repo/remote state invariance and no dispatch; classifier pins for `push`/`pull` join the existing `fetch` pin. The suite stops claiming uncontained remote Git succeeds, without treating the unavailable capability as a pass-through success. Closure in `plans/closure/git-reset-remote-test-contract-ci-corrective/002-status.md`.

## Dependency graph

- C001: no hard dependencies (typed policy closed under Linux C001 + clean C001 pattern). Operational: hosted CI.
- C002: no hard dependencies; **interface** on Linux C003 for the positive-path contract (blocked: C001, M005/M006, transport policy). C002's fixtures must keep failing closed if C003 later lands a real backend — i.e., they assert current unavailability and cite C003, and C003's landing must update them rather than coexist with a stale unavailable assertion.
- Neither milestone unblocks Linux M001/C002/C003, ADR-0027, or any other line; both are leaves that discharge the `git_network_integration` red owned by this branch.

## Non-goals, migration, qualification

No new Git operation or schema, database migration, tool permit, remote transport, credential broker, symlink traversal relaxation, Linux kernel isolation promotion, or permission redesign. No `#[cfg(test)]` bypass, fabricated grant, fake executable, FullHost default, or classifier weakening. Recheck code and latest CI before implementation; two repos/branches advancing must not create conflicting changes. Verify `tests/git_network_integration.rs`, policy-classifier tests, Git origin matrix, safe local Git operations, guards, and hosted CI. If the typed policy is demonstrably wrong relative to accepted ADR or real trusted authorization is already present but dropped, stop and register a focused **production authority plumbing** successor; never silently disable capability checks for these tests.
