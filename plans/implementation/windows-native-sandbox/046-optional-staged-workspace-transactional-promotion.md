# Windows Native Sandbox C046 — Optional isolated workspace and transactional promotion

Status: blocked pending separate staging selection / evidence; historical C015–C018 remain blocked (closure `plans/closure/windows-native-sandbox/046-status.md`)
Class: capability (conditional)
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0029 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0029 (bounded test-only), proposed ADR-0030 is NOT product authorization.
Dependencies: C043 or C044 candidate evidence showing stage required; new accepted staging architecture decision; C042 contract hard
Closure target: plans/closure/windows-native-sandbox/046-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective

Offer a separate workspace materialization/promotion backend only if the selected native policy cannot safely execute tools against the real worktree. Do not silently reopen ADR-0019's AppContainer C015–C018 line; this plan is a new scoped design successor.

## Work packages

A. Determine an immutable snapshot/worktree manifest and bounded quotas for content, symlinks, hardlinks, NTFS alternate data streams, junctions/reparse points, source file identity and workspace dirty-state. No CopyFileExW metadata/DACL copying, broad host credential/cache access, original-root ACE edits or unverified open-by-path races. Clearly specify how .git, submodules, Git hooks, ignored/build files, package caches and concurrent worktrees behave.
B. Materialize independent stage objects with runner-owned identity, enforce stage read/write and original host denials through the qualified backend, and benchmark Git/PowerShell/Python/Cargo representative workloads. Startup cost must be measured; persistent stage reuse needs versioned provenance and freshness checks rather than trust in a mutable path.
C. Inventory outputs/renames/deletes and preview diffs under daemon-owned authorization. Promote only approved changes with hash/identity/freshness compare-and-swap, atomic file replacement where possible, durable journal, rollback, malicious symlink/hardlink/swap prevention and remote/multi-client conflict tests.
D. Define cancellation, crash/restart, staging directory cleanup and per-run quotas; avoid installing a parallel unmanaged executor or bypassing Codegg's existing git/project mutation control.

## Acceptance / stop

A genuine isolated-running-child deny matrix and allowlist, Git/toolchain compatibility, exact input/output identity, safe concurrent promotion and restart rollback must pass before any user-facing staged capability. If an unsupported workload requires copying real secrets, broad host grants or weakening original ACL invariants, stop and record negative. Never interpret successful copy/promotion as proof of read confinement; network still separate.
