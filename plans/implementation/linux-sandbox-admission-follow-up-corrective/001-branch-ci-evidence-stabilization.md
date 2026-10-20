# C001 — Linux Branch and CI Evidence Stabilization

Status: **ready** (corrective infrastructure; no production admission)
Source: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`, C001; original Linux M003/C010 and C005 no-admission remains. Baseline: `planning/linux-sandbox-next-admission-and-adr-cleanup` `d251b11cd7fa5b89ca008e45d80a28a4ac3aea9f`, 2026-10-20.
Closure target: `plans/closure/linux-sandbox-admission-follow-up-corrective/001-status.md`.

Authority: long-term §§4/17/27/29; `plans/003-planning-process.md` §§4–10; accepted ADR-0004, ADR-0016, ADR-0042, ADR-0043. Parent implementation `plans/implementation/linux-execution-isolation/008-qualification-and-rollout.md` and closure `plans/closure/linux-execution-isolation/018-status.md`.

## Objective and evidence gap

Make the existing long-lived Linux candidate **reviewable and reproducibly testable** against current `main`, without merging it to production or implicitly closing any capability. At review it was 295 commits ahead/90 behind. The `d251b11c` record reports a quiescent 14,877-test sweep with three 120-second LSP timeouts and seven skips, plus an earlier fourteen-test intermittent Git/security-review failure sharing process-cwd Git prerequisites. Repro attempts did not identify a root cause. A successful M004/M005 hosted run at `862ddd14` is narrower than all-source CI. The branch contains `plans/.mutate-probe`; its provenance needs examination.

The earlier testing did not catch the remaining failure deterministically because tests use shared process/current-repository state, timeout behavior under load differs from isolated runs, and focused native workflow successes do not cover full workspace/merge conflicts.

## Scope, owner and non-goals

Own branch reconciliation/evidence, test-fixture isolation and CI guard anti-vacuity only; no Linux security profile/default decision or M003/C010 admission. Do not redesign Git semantics, restart M004, requalify C007 code as if missing, change arbitrary runtime timeouts, discard user changes, or merge the entire long-running branch into `main` as part of this plan.

The agent may stage reconciliation in a separate integration worktree/branch based on an **explicit captured pair of SHA heads**, preserving the source branch and historical closure records; split mechanical integration from behavior changes. Nontrivial unrelated conflicts belong to their subsystem owners and must be listed rather than silently resolved by deleting assertions.

## Ordered work packages

1. **Freeze provenance:** record Linux source and `main` heads/merge base and compare full diff by subsystem. Inventory Linux-only source, documentation, workflow/test changes, unrelated changes, ADR canonical identities, migrations (including v84), and whether `plans/.mutate-probe` is required or stray. Remove an extraneous probe **only with provenance/test confirmation**, never by guessing. Record exact CI runner successes/failures and revision.
2. **Construct an integration candidate:** merge/rebase current `main` into an isolated worktree with no force push or loss of parallel work. Inspect every conflict in process/approval/network DTOs, Git publication and scheduler payloads. Preserve accepted ceilings, C007 journal/migration, C014/C023 obtained facts, and current mainline features. If clean conflict resolution cannot be completed within this bounded handoff, document the conflicting files and stop; do not claim mainline-ready.
3. **Eliminate hidden test-state assumptions:** the four Git-routing and ten security-review tests must create/own a temp Git worktree or explicitly bind a scoped repository cwd per test; do not rely on mutable shared process cwd or host repo state. Reproduce with true concurrent tests and temporary repositories. Preserve legitimate typed `BackendUnavailable[RepositoryAlias]` refusals; no blanket widening of Git subcommand authorization to make tests pass.
4. **Resolve LSP timeout ownership:** instrument elapsed phase and contention for the three known load-sensitive LSP tests, retain normal functional assertions, prefer per-test service/workspace fixtures or a bounded scheduling fix. If the cause remains unknown, mark **conditional** rather than lowering the timeout, disabling or silently excluding tests. Do not make unrelated bulk changes merely to produce green CI.
5. **Non-vacuous guard and reporting:** ensure security test filters actually execute named tests; assert count, no self-skip marker and positive/negative controls. Identify workflow path-trigger omissions in `.github/workflows/linux-m004-native.yml` so Linux owner code changes trigger native qualification. Record scope for `.github/workflows/linux-c006-native.yml` and macOS compile.
6. **Review pack:** provide a small per-boundary changed-file/commit ledger, exact `main` merge baseline, migration/ADR integrity result, CI qualification matrix and recommended smaller review chunks. No direct release/admission action.

## Verification and required evidence

Run `git diff --check`, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `scripts/verify.sh quick`, `python3 scripts/check_adr_integrity.py`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_sandbox_policy_wiring.py`, `python3 scripts/check_network_intent_owner_wiring.py` if present, and `python3 scripts/check_git_forbidden_patterns.py`.

Run the affected `cargo test --locked --test git_execution_origin_matrix`, `cargo test --locked --test command_routing_execution_ownership` and `cargo test -p codegg --lib security::workflow`; exercise focused Bash route tests by exact names and retain legitimate typed-negative cases. Run `cargo nextest run --workspace --locked --profile ci --no-fail-fast` at an **unchanging integration revision**, and separately a native M004/M005 Linux hosted job, protected-mount candidate job and macOS target compile. Preserve exact runner links; capture 0-match/skip counts; do not infer unrun platform behavior.

Add regression fixtures demonstrating no process-cwd/shared-Git dependence and no vacuous security filter. Document timeouts as timeouts. A fully green full suite is the desired integration exit, but not license to alter unrelated product policy; an unresolved intermittent failure produces conditional/blocked closure with reproducer and named owner.

## Exit / stop / closure

**Pass:** clean (non-destructive) integration candidate, every required focused check genuinely executed with positive controls, branch-wide test outcome and remaining known failures classified, links to source and tested heads, no false admission/default/ADR claims. **Stop:** unreconciled security/authorization semantics; unsafe branch rewriting; evidence suppression; unexplained fixture edits hiding typed denial; lost v84 migration or journal. Distinguish readiness from actual merge: no production merge is authorized here.

Closure `001-status.md` must include requirement-to-evidence matrix, before/after flaky test signatures and measured test counts, workflow/run URLs, git-tree and migration/ADR reconciliation, security/compatibility effects, and an explicit list of remaining blockers with owners.
