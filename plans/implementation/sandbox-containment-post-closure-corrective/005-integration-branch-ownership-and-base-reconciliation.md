# Sandbox Containment Post-Closure Corrective C005 — Integration Branch Ownership and Base Reconciliation

Status: implemented (conditionally closed; see `plans/closure/sandbox-containment-post-closure-corrective/005-status.md`)

Repository baseline: `planning/sandbox-containment-post-closure-corrective` at `e6950f5088dc1bb855c4c3c27ac7507b67fa07fd`; `main` and merge base `7317a67ad604fe39fd5282ddca07496255faa4d1` (observed 2026-10-12). Re-evaluate refs before implementation; the branch is caught up but still mixes workstreams.

Source roadmap:
- `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Long-term requirements:
- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Applicable ADR: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md` (do not silently relax sandbox authority).

Predecessors and closure evidence:
- C001 closed: `plans/closure/sandbox-containment-post-closure-corrective/001-status.md`.
- C002 closed: `plans/closure/sandbox-containment-post-closure-corrective/002-status.md`.
- C003 closed: `plans/closure/sandbox-containment-post-closure-corrective/003-status.md`.
- C004 conditionally closed: `plans/closure/sandbox-containment-post-closure-corrective/004-status.md`.
- PR #110: https://github.com/dbowm91/codegg/pull/110 .
- Native Apple Silicon evidence: https://github.com/dbowm91/codegg/actions/runs/38166386985 ; Linux Landlock ABI 4 evidence in the C002/C003 closures.

Primary class: invariant (merge ownership, traceable security evidence); infrastructure (integration).

## 1. Objective

Convert the mixed multi-workstream development branch and stale planning-only PR into a reviewable sandbox-containment change based on current `main` **without losing or rewriting** the independent human-shell and editor implementation. Reconcile shared source/planning files consciously and preserve the actual security tests and previously qualified implementation. The output of C005 is a correct integration diff and declared PR/branch ownership, *not* automatic strict closure of C004 or an unconditional merge. A clean rebase without scope separation and preserved planning files is not completion.

## 2. Why ready

C001-C003 and the qualified portions of C004 have implementation and closure evidence. C004's unqualified Apple Events/Launch Services, XPC/Mach and privilege-transition probes are explicitly **not** prerequisites for merging the documented filesystem write/credential-deny contract, so long as that limited contract remains correctly stated. No other technical hard predecessor is missing. This plan addresses the new integration defect exposed after those closures; do not reopen the closed core mechanism solely to have work to do.

## 3. Current implementation and source of integration defect

GitHub compare `main...planning/sandbox-containment-post-closure-corrective` now returns `ahead`, **37 ahead / 0 behind** at the recorded baseline. A branch reconciliation made the earlier C005/C006 plans and registration from commit `baf9f228` unreachable on the active branch; this is a demonstrable planning continuity defect. PR #110 is still open with obsolete *planning only* metadata and historical ready/blocked milestone statuses.

Despite zero commits behind `main`, the branch still carries at least three separately accepted workstreams in its proposed diff:

1. **Sandbox C001-C004** — `src/security/sandbox.rs`, `src/security/sandbox/{policy,landlock,seatbelt}.rs`, `src/python_script/{sandbox,executor}.rs`, `src/managed_process.rs`, `src/tool/bash/process.rs`, sandbox tests/guards, security/Python docs and corresponding plans/closures. Key source commits `f4560665`, `be7537cc`, plus qualification corrections `875e28b8`, `98f96a99`, follow-ups through `52bc62fc`. C001-C003 are **closed**, C004 **conditionally closed**.
2. **Human shell** — `crates/codegg-core/src/session/human_shell.rs`, `crates/codegg-protocol/src/human_shell.rs`, daemon authorization/persistence/transport, `src/shell/human_dispatch.rs`, TUI `!` dispatch, human-shell plans and closures, migration v71. Independently closed C004 implementation `900d1ec5` and C002 implementation `f9d999de` are documented under `plans/closure/human-execution-corrective/`.
3. **TUI editor / LSP** — `src/core/daemon_lsp.rs`, `src/tui/commands/editor.rs`, editor rendering/state, desktop frontend roadmap and `plans/closure/desktop-frontend-ide-foundation/010-status.md` (M006-C implementation `359bf291`).

Shared `plans/registry.md`, documentation and potentially `src/managed_process.rs` / daemon execution ownership must be audited hunk-by-hunk. Do not assume that paths belonging to another workstream can simply be reverted or that commit boundaries align perfectly with file boundaries.

PR #110's **original planning-only scope is invalid**, even though implementation and native-host evidence exists. Neither rewriting #110's body alone nor a quick green check proves that unrelated workstream ownership is correct.

## 3A. Concrete failed integration checks and ownership boundary

The last checked PR-head runs on commit `e6950f50` are **failing**, not merely absent:
- [CI / verify 38177378810](https://github.com/dbowm91/codegg/actions/runs/38177378810), job 114582507146: formatting, sandbox and ownership guards pass, but workspace Clippy `-D warnings` fails with eight errors: `src/core/daemon_turns.rs` (two `result_large_err`, two `needless_return`, one `unnecessary_to_owned`), `src/tui/commands/editor.rs` (two `useless_conversion`), and `src/core/daemon_lsp.rs` (one `items_after_test_module`). Workspace tests are skipped after the failure.
- [Desktop E2E 38177378815](https://github.com/dbowm91/codegg/actions/runs/38177378815), job 114582507440: `apps/desktop/src-tauri/src/present.rs` around lines 462–479 duplicates `input_tokens` and `output_tokens` initializers (two `E0062` errors). Built-app WebDriver trajectory is skipped.

C005 must classify these files and code changes under their **actual** human-shell, LSP/editor, or desktop owners. Do not make unrelated feature edits on a sandbox-only PR just to quiet CI. C006 owns targeted per-owner remediation and final hosted qualification once the integration topology is established. A CI failure is not a runner flake merely because its code is from an adjacent workstream.

## 4. Non-regression invariants

- No sandbox regression: helper-only child confinement, `Required` fail-closed semantics, Landlock ABI>=3 TRUNCATE rights, `no_new_privs`, nested/future credential denials, Seatbelt final-deny ordering, scoped private scratch lifecycle, Python Analyze/Verify/Transform authority.
- Preserve the documented *differences*: Seatbelt broad reads, unrestricted network, inherited descriptors, residual IPC/privilege limits. Do **not** promote C004 to strict closed.
- Preserve separately accepted human-shell staging/runtime, migrations, and authorization, as well as M006-C editor changes, **without accidentally deploying them under an unreviewed sandbox PR**.
- Keep accepted original implementation SHAs, closure evidence, and final diff provenance traceable. No blind `git reset --hard`, force push, squash/rebase over active concurrent work, or mass revert of other agents' implementation.
- A PR's title/body must describe the actual diff reviewed by maintainers. A mergeability indicator alone is not a security review.

## 5. Scope

**In:** commit graph and per-hunk provenance audit; independent workstream ownership disposition; safe reconciliation with updated `main`; PR/branch scope repair; focused sandbox and shared ownership guard regressions after integration; update current planning index without rewriting prior closures.

**Out:** new sandbox rights, stronger Seatbelt read/IPC restrictions, network sandboxing, code changes unrelated to conflicts, adding another scheduler/human-shell feature, GUI/editor features, protocol/schema migrations, new CI matrix, deleting/recreating branch history to make a pretty diff, merging before C006 final qualification.

## 6. Required integration changes and work packages

### A. Freeze an immutable inventory and candidate diffs

Record current HEADs of `main`, work branch, merge base, PR #110, and workstream implementation/closure SHAs. Inspect:

```bash
git fetch origin --prune
git log --graph --oneline --decorate origin/main...origin/planning/sandbox-containment-post-closure-corrective
git rev-list --left-right --count origin/main...origin/planning/sandbox-containment-post-closure-corrective
git diff --name-status origin/main...origin/planning/sandbox-containment-post-closure-corrective
git diff --check origin/main...origin/planning/sandbox-containment-post-closure-corrective
```

Create a compact ownership matrix of every changed file/commit: sandbox, human-shell, editor, shared, unrelated, or unclassified. For shared hunks, document owners on both sides and their final behavior; no unclassified source change may reach a sandbox-only PR. Check if independent work is already in `main` or other open PR heads; do not duplicate/revert it blindly.

### B. Select a preservation-first integration topology

**Preferred outcome:** one sandbox-specific reviewable PR containing only the sandbox changes and necessary shared dependencies; other workstreams retain their own branches/PRs/evidence. Choose the smallest safe method based on the actual ancestry:

1. If `main` already contains the accepted human-shell/editor commits by the time work starts, update work branch with `main`, ensure those changes disappear from the PR diff, and retain #110 as the sandbox integration PR.
2. Otherwise, preserve the existing mixed branch as a durable provenance branch; create a **new sandbox-only integration branch from current `main`** and selectively transplant sandbox-owned commits/hunks without overwriting other workstreams. Open a replacement focused PR and explicitly link/supersede #110. Never silently delete independent commits.
3. If one or more sandbox changes have real hard runtime dependencies on still-unmerged human-shell/editor commits, name each dependency and choose a safe explicit base/order (or an intentionally stacked PR with the correct base). An arbitrary cross-workstream merge into main is **not** an acceptable shortcut.

Do not force-push PR #110 to a different ancestry while independent human-shell/editor development remains on it. GitHub PR head branches are not retargeted by simply changing the title; if a replacement branch is needed, preserve links and close/supersede the old PR only after the replacement is published and verified.

### C. Reconcile with the current base safely

Merge (or use a non-destructive, ownership-qualified transplant from) the latest `main` onto the chosen integration branch. Resolve conflicts in `src/security/sandbox*`, Python, `src/managed_process.rs`, `src/tool/bash/process.rs`, `plans/registry.md`, execution-ownership manifests/guards, and docs by preserving both owners' accepted behavior; never choose one entire file side on security/authority conflicts. Identify schema version/migration order if shared daemon files remain touched. Capture merge conflicts and their resolution rationale. Ensure branch behind count is zero at submission, or document a newly advanced base and repeat short verification.

### D. Verify resulting diff and ownership boundaries

Inspect **both** `git diff <base>...HEAD` (review diff) and integration-branch history. Confirm the sandbox PR contains no human-shell or editor feature implementation unless an accepted and documented hard dependency requires it; if so, use separate prerequisite PR and explicit ordering. Confirm focused backend/probe tests from C001-C003 and static guards still exercise production source after reconciliation. Confirm PR metadata/links accurately name C001-C004 as implemented, C004 conditional, and the host-specific limits. Leave normal hosted sweep and final closure for C006.

## 7. Failure, cancellation, recovery, contention

Integration can be interrupted at any step. Keep original branch refs and content available so recovery is a branch checkout rather than reconstructing commits from a partially force-rebased tree. A failed cherry-pick or merge is a **stop-and-resolve** event; do not force merge or drop code to greenlight checks. When multiple agents share the original branch, coordinate branch ownership before moving its head. New sandbox-only branch can be prepared independently and pushed, preserving concurrent work on the mixed branch.

No runtime storage/protocol migration is introduced by this corrective itself. Any retained independent migration (e.g., human-shell v71) must be separately authorized and owned by its own PR. Security and process cancellation semantics remain as verified by C001-C003.

## 8. Focused verification after reconciliation

```bash
git diff --check
git rev-list --left-right --count origin/main...HEAD
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo fmt --all -- --check
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --lib python_script::sandbox
cargo test -p codegg --test sandbox_containment -- --test-threads=1
scripts/verify.sh quick
```

Use actual available guard self-test flags and focused targets; skip `--self-test` if the guard does not implement it. Run once on the final local integration candidate. Reuse the accepted real macOS and Linux evidence if sandbox implementation hunks are unchanged, and document why it applies to this diff; if policy/helper semantics changed during conflict resolution, repeat the corresponding native focused fixture before C006 can accept it. Avoid a broad duplicated test matrix solely for PR tidying.

## 9. Acceptance

- The candidate sandbox PR diff is bounded to the accepted C001-C004 work and explicit, justified dependencies; every changed file/hunk has an owner and documented disposition.
- Existing human-shell and LSP/editor changes and associated accepted records remain reachable and independently reviewable; no dropped or silently reverted production changes.
- The candidate integration branch is based on up-to-date `main` without unresolved conflicts; no force history rewrite was used on a shared branch.
- PR #110 is truthful and reviewable **or** is preserved and superseded by a clearly linked sandbox-only PR. No stale "planning only" claim remains on the active sandbox submission.
- Focused sandbox/provenance/ownership guards pass (or a concrete failing invariant blocks closure), and the accepted Linux/macOS test provenance remains valid.
- `plans/registry.md` and the sandbox roadmap identify C005 closure and C006 eligibility without changing C001-C004 historical facts or the C004 conditional security status.

## 9A. Planning continuity and preservation requirement

Before cherry-picks, rebases, replacement PRs, or scope separation, ensure the C005/C006 documents, roadmap/registry references, and prior closure records are included in the **surviving** integration branch or safely preserved successor. The previous disappearance of C005/C006 from the active branch is a failure of handoff continuity even if `main` is fully integrated. Review the resulting PR diff against the restored documents. Verify no human-shell migration, desktop/editor implementation, or branch tips are accidentally dropped. Provide a file/commit ownership matrix for the four CI-failing code paths above and name how each is repaired or removed from the sandbox submission.

## 10. Stop conditions

Stop and report an exact commit/file/hunk when independently owned work cannot be disentangled without a semantic redesign; a merge conflict changes the sandbox/human authority contract; current `main` unexpectedly diverges behind the sandbox base; a shared branch is concurrently changing without an agreed lease; a previously accepted human-shell migration would be dropped; or focused security regression fails. Do not create a third reconciliation plan or claim closure without explaining the observed blocker.

## 11. Closure evidence and handoff

Write `plans/closure/sandbox-containment-post-closure-corrective/005-status.md` with before/after refs, change inventory and ownership matrix, actual chosen branch/PR topology, preserved sibling branch/PR links, conflict resolution details, diff/commit checks, focused commands and outcomes, requalification decision, severity-ranked issues, and explicit C006 handoff. Update roadmap/registry. C006 is hard-blocked until this record establishes a stable reviewed integration candidate; no new production feature follows from C005.
