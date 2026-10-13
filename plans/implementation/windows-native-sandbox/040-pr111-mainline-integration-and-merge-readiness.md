# Windows Native Sandbox C040 — PR #111 Mainline Integration, Registry Conflict, and Merge Readiness

Status: **implemented; closed**; C039 ADR-identity reconciliation is closed
Primary class: integration/quality corrective
Refreshed integration baseline: feature parent `a00a766af7ebdd7db770182ac47e4ddc824dd9d8`; `main` `b9ba494c1b21638c2e2f660301ba207ce8859682`; common base `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`; merge result `b89ba9e364bbf492d19048bed54803c4a8fd6d8e`.
Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#post-adr-0024-pr-111-mainline-reconciliation`.
Authority: `plans/003-planning-process.md`, ADR-0004, ADR-0016, no-admission decisions C029/C034/C038. PR <https://github.com/dbowm91/codegg/pull/111>.
Hard predecessor: C039 recorded collision-free ADR reconciliation on this branch or a maintainer-approved equivalent migration. Soft/parallel predecessor: shell-policy C001 exact-head tests.
Operational: non-shared integration worktree and working Linux/Windows CI; no permission to auto-merge a draft PR.

## 1. Verified merge audit on planning date

GitHub marks PR #111 `mergeable_state=dirty` / `mergeable=false`; it is a **draft**, with 159 commits and 108 changed files. Comparing the known fork base to current `main` finds **8 mainline commits / 98 changed paths**; the feature branch has 159 commits / 108 changed paths. The **only exact path modified on both sides is `plans/registry.md`** (a content conflict at its active-subsystem table near base line 28). Mainline adds TUI editor/directory listing planning and implementation; the branch adds Windows security research and shell-policy plan registrations. The branch is not behind its fork point, but it *is* missing eight new mainline commits.

There is a **second semantic conflict that Git cannot flag**: mainline `ADR-0021-native-editor-completion-read-and-local-edit.md`/`ADR-0022-daemon-authorized-workspace-directory-listing.md` versus branch's distinct Windows ADR-0021/0022. C039 owns the identity migration; do not "resolve merge conflicts" while leaving that duplicate-numbering issue.

A separate useful discovery: mainline commit `9d32c35b516315adef21e30eb2a9e436826231e0`, titled "test(bash): rewrite the stale blocklist tests around the two-tier policy", **already replaces** the two historically failing tests on `main`. The feature branch still contains the old tests. C001 now needs the upstream test change **integrated and validated**, not independently rewritten from scratch.

## 2. Non-goals and invariants

Do not rewrite, force-push or discard branch history, delete mainline TUI/editor correctness work, silently pick "ours" or "theirs" wholesale for `plans/registry.md`, or conflate native test-only Windows feasibility with released sandbox support. Do not enable `SandboxRequest::Required`/`Enforced`, install a helper/broker/guest, change original host DACLs, close M004–M006, reopen C030/C031 or reverse C038's no-candidate decision. Do not merge PR #111 into `main`: the user requested preparation and handoff only. Work must remain on the existing Windows feature branch, using a clean **separate integration worktree**.

## 3. Ordered implementation sequence

**A — Snapshot and preview, no mutation.** Fetch exact current `origin/main` and feature ref. Confirm common base and compare path sets with `git merge-base`, `git diff --name-status <merge-base>..origin/main` and `git diff --name-status <merge-base>..origin/work/windows-native-sandbox-foundation`, plus `git merge-tree` before choosing a strategy. Record newly overlapping paths; initial evidence says only `plans/registry.md`. Keep the current feature branch untouched until preview and resolution recipe are approved; create a dedicated clean worktree and record its commit SHAs. Include C039's namespace audit result.

**B — Reconcile registry semantically.** Integrate current mainline into the feature branch with an **ordinary explicit merge commit** (preferred over rebasing 159 published commits; no force push). Resolve `plans/registry.md` by preserving **both** groups of entries: mainline TUI editor/directory listing readiness and closures, Windows C001/C013–C038 negative/blocked roadmap and ADR authority, and new C039/C040 state. Use the current mainline row content, not stale snapshots. Check row uniqueness, status vocabulary, references and section ordering; ensure the Windows table is not silently overwritten by mainline and TUI work is not lost by Windows branch. Re-run conflict inventory if mainline changed since the baseline. Avoid accidental end-to-end "all green" statements.

**C — Bring forward the already-landed shell-policy fix.** Verify the merged tree contains the mainline test rewrite from `9d32c35`, including distinct hard-deny and Ask tiers in `tests/tool_execution.rs`. Validate that `PermissionChecker.check_bash_legacy` actually returns `PermissionResult::Ask` for `sudo su` / `echo $(whoami)`, and hard-deny examples do not execute even with permissive user rules. The existing branch's failing `test_bash_tool_blocked_patterns` / `test_bash_tool_blocked_command_list` assertions should no longer be present as stale requirements. If a real approval bypass is discovered, stop and register a separate security corrective; do not bury it in a merge conflict resolution.

**D — Broader cross-surface regression.** Run `git diff --check`, `cargo fmt --all -- --check`, focused `cargo test --locked --test tool_execution` and `cargo test --locked --test permission`, `cargo test --locked --test windows_native_probe_contract` (Windows native runs where required), `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `bash scripts/verify.sh quick`, and the repository's full hosted Linux CI. Use exact-SHA required Windows process launcher and native probe contract workflows, distinguishing historically negative opt-in C033/C035/C036/C037 evidence jobs. Check that TUI editor/directory-listing tests from current `main` still run and pass. Any skipped native work is marked **not_run**, not pass. A green opt-in research job is not equivalent to sandbox security qualification.

**E — Final draft PR review and closure.** Verify the update is fast-forwardable, merge was non-forced, `git merge-tree` and GitHub PR mergeability show no unresolved conflicts, registry includes both workstreams, ADR IDs are collision-free, CI checks match **new head**, and the branch still reports production Windows containment unavailable. Inspect PR changed-file scope against baseline to catch accidental code churn, stray shell artifacts or removed Windows historical closures. Create `plans/closure/windows-native-sandbox/040-status.md` with conflict resolution summary, exact main and branch merge SHAs, row/link count, CI links, security regression truth and residual blockers; update registry/roadmap and shell C001 status. If PR #111 remains draft, say so—do not mark release-ready or merge.

## 4. Security, compatibility, and operational stop rules

Mainline integration must retain managed-process single ownership, Job supervision/fail-closed mode, Linux Landlock and macOS Seatbelt semantics, approval ceilings and Windows native test-only boundary. No production configuration change or credential/ACL migration permitted. Preserve new mainline TUI improvements and do not introduce additional direct command execution paths. No Windows guest/service/user credential provisioning is performed here.

Stop if: merge introduces unreviewed changes to production sandbox backend, blocked M004 turns ready without proof, ADR collisions remain, branch's C035/C036/C037 negative evidence is changed to green, the historical shell test fix was lost, any required CI failure is hidden, or the branch cannot be updated by a non-forced push. In that case record blocker rather than force the PR merge.

## 5. Acceptance and handoff

- C039 prerequisite yields globally unique active ADR IDs and traceable Windows record identities in the eventual merged tree.
- `origin/main` features and windows branch evidence are both present, with no unresolved registry conflict; `git merge-tree` and GitHub `mergeable_state` permit merging (other repository checks may still prevent final merge).
- Upstream `9d32c35` shell tests are integrated; shell C001's owner-level approval/hard-deny tests and hosted root CI are confirmed on the **same head**, not a different branch.
- Windows production `Required` remains unavailable and filesystem/network `Enforced` unclaimed. No original-user ACL edits or negative-evidence reinterpretation.
- PR stays draft unless separately authorized; no merge into main, no force pushes or history truncation.

Run one coherent integration pass, produce closure and links; avoid merging a stale snapshot solely to make GitHub show a green merge badge.
