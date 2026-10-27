# Harness Lab Mainline Qualification H001 — Integration and Exact-Resulting-Head Verification

Status: **ready**
Baseline reviewed: `main@957fcbf00e17bf108429f0d75dcbf760736ae511`; continuation implementation `56279dfc7dc8ee82f0239d6412b4fa50a42a74d6`; continuation head `cd9fde42491c85941c31efbc2d5d200e8db4d638` (2026-10-27). Refresh all refs and outstanding PRs on entry.
Source roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`. Long-term references: `plans/000-long-term-specification.md` §§23,26,29; `plans/003-planning-process.md`; original D001–D003 and E001/E002 closures. Primary class: invariant. No ADR required unless merge reveals authority change.

## 1. Objective

Make the proven continuation fixes actually available on `main` while preserving an accurate qualification boundary. The continuation source passed hosted `CI / verify` at `56279dfc7dc8` (run `40383901325`); the two later commits were closure and a separately authorized 63-trial event-gap sample. That **does not** establish a green result on the eventual merge/rebase head, and the continuation branch had no PR at review.

This plan is for a reviewable conventional integration, not an instruction to silently merge, automatically push unreviewed candidates or reopen closed Harness Lab features.

## 2. Scope and invariants

- Integrate D001 rooted `exec`/`run` workspace registry and path policy, D002 typed tool terminals through ACP, D003 sustained loss-aware observation, Recovery C002 progress evidence cleanup, and E001/E002 CI/SQLite corrections with their regression tests and closure provenance.
- Keep user edits on `main` and unrelated active branches, including TUI subagent/job visibility, sandbox authority, existing tool-program behavior and Windows/Linux qualification workflows.
- Preserve strict `-D warnings`, tool broker auth, `workspace-write`/Deny defaults, path-root guard, failure/outcome truth, ACP v1 compatibility, event-gap unmeasured semantics, no rerun of acknowledged mutating calls and no changes to historical ledger receipts.
- The `opencode_go` adapter is the **only** provider integration in this follow-up; no upstream-provider parity or extra CodeGG provider integration belongs in CI/integration gates.
- No new model-backed live trial, no prior operator budget reuse. PR premerge tests remain offline. Prior live sample applies only to its pinned binary/host/model mix, not automatically the new merge build.

## 3. Dependencies and ownership

Ready: existing continuation D001–D003, Agent Recovery C002, CI E001/E002 all closed, source branch and merge base available. Soft coordination with H002/H003 because their new fixture work may land alongside, but **do not** hold product integration hostage to the optional long-run pilot H004. If main has materially changed, reconcile independently and rerun exact resulting-SHA tests.

## 4. Ordered handoff

1. **Audit source/PR state:** capture updated main tip, continuation tip, common ancestor and actual changed paths. Verify D001–D003 code exists, C003 original candidate is no longer a substitute for mainline source proof, and C006 live receipts remain immutable. Check duplicate work for each touched file on current main. Review source against `plans/closure/harness-lab-continuation/001..003-status.md` and `plans/closure/ci-exact-head-signal/001..002-status.md`.
2. **Create normal integration review:** prepare an integration worktree/branch with continuation commits reconciled to main using a merge/rebase chosen for auditability. Open a PR with requirements, behavior diff, known historical limitations, CI evidence and no “all providers” claim. Do **not** force push main or auto-merge. Keep existing continuation branch as traceable historical source.
3. **Reconcile conflicts by ownership:** registry/roadmap must preserve both sides; `src/exec.rs` and tool registry authority cannot regress; ACP projection must keep D002 typed truth and C006 gaps; C002/ADR-0040 semantics cannot regain an unreachable `Stalled`; tests should retain both independently added negative controls. Do not mechanically choose one branch's tests and lose the other.
4. **Exact candidate verification:** run locally where feasible `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`, full workspace tests, Harness Lab Python, test `exec_workspace_path_policy`, corrupted M015 journal/SQLite typed lock, recovery observation guard, ACP tool failure/gap, and a two-private-lane native daemon+ACP smoke. Record counts, skipped, infrastructure failures, and source/binary SHA.
5. **Hosted gate:** confirm all required `CI / verify` and applicable platform workflows passed **on the exact integration head**, or explicitly classify an unrelated gating failure. The earlier 40383901325 pass is only historical. Follow repository review policy for maintainer merger authorization; if merged, rerun/confirm the **resulting main merge SHA** and branch ancestry.
6. **Negative boundaries:** remove fixture provider secrets from all lanes; explicit Deny and out-of-root refusal; one daemon stopped with the other still operational; no duplicate M015 job resume; ACP failed tool remains failed; no scope escalation during merge.
7. **Reporting:** write `plans/closure/harness-lab-mainline-qualification/001-status.md` with PR number/merge SHA, exact-run URLs, changed files, tests/guards, compat/security table, and any unmerged blocker. Advance registry only when merged and the target `main` head is actually qualified.

## 5. Failure and stop conditions

- Rebase conflicts crossing security, policy or producer ownership: stop, report responsible codeowner/ADR and isolated failing-first test; don't settle by broadening authority.
- GitHub workflows fail for independent reasons: keep H001 **conditionally ready/blocked**, not closed; retain exact failure paths/SHAs and owner.
- New CI full suite takes significant time: no SBC build/startup test; ordinary hosted CI and developer workstation only. No new permanent CI lane.
- PR isn't reviewed/merged: closure must say “review-ready” or “conditionally closed,” **not** “landed in main.”
- A later H002/H003 change modifies production source after successful CI: requalify exact new SHA.

## 6. Acceptance evidence

A reviewed integration PR; source diff and ownership table; genuine `main` merge commit; strict exact-target CI success; proof of 2+ independent daemon subjects, rooted headless tools and Deny, truthful ACP failure, event gaps unmeasured, safe M015 restart/cancellation, and preserved legacy tests. Never accept on a planning-only commit or on branch-only CI alone.
