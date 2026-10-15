# Windows Native Sandbox C051 — Disposable-stage toolchain and promotion-contract feasibility

Status: closed blocked — C050 strict-positive qualification gate was not met; see `plans/closure/windows-native-sandbox/051-status.md`
Class: infrastructure (test-only integration, not production staging)
Baseline: main aa6cbcb1e154a2676325fb654c5b68dc068edfc3 (refresh at execution).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (post-C043/C044 successor).
Authority: ADR-0004, ADR-0016, ADR-0029 bounded research. ADR-0019 C015–C018 remains blocked, C046 not unlocked; proposed ADR-0030 unapproved.
Dependencies: C050 strict-positive *write* confinement and verified original-host no-mutation (hard); C049 policy matrix interface; an approved separate staging ADR is hard for any non-fixture/user-data materializer or production promotion.
Closure target: plans/closure/windows-native-sandbox/051-status.md.

## Objective

On a proven C050 test-only token, determine whether an independent, newly created NTFS stage can support everyday Windows Git/Python/Cargo operations economically, and specify the trustworthy input/output handoff contract. C050's disposable canaries establish token semantics; C051 adds realistic *synthetic repos only* and stage-lifecycle evidence. It MUST NOT implement a production stage or authorize C046 by default.

## Work packages

A. Define an isolated synthetic repository fixture with representative Rust crate, Python venv, Git files/objects/submodules/hooks and PowerShell or cmd tasks. Account for Windows original-worktree .git indirection, linked worktrees, junctions, ADS, hardlinks, shared caches, temp paths, build artifacts, toolchain access and hostile hook/helper child execution. Explicitly identify where copying from an actual host root would require an independently approved identity-safe snapshot, rather than treating git worktree as isolation.

B. Create stage/scratch roots anew with C050-qualified ACL/token policy; populate from **synthetic test-owned files** using separate file objects, not inherited original-host security descriptors. Reject or enumerate reparse/ADS/hardlink cases; attest staged inputs by handle-based file identity and hashes. Run real native Git init/status/add/commit, Python virtualenv/pip offline fixture, Cargo offline check/build/test with dependencies preinstalled, PowerShell and cmd on supported x64/ARM64 hosts when available. No network restrictions asserted.

C. Collect warm/cold costs, cache miss/hit behavior, stage copy overhead, per-command startup and process-tree teardown under cancellation/crash/restart; compare native uncontained controls. Simulate two sessions editing same synthetic source, then produce a **promotion contract/design and dry-run conflict report only**: versioned manifest, source hash/identity, compare-and-swap, authorized diff preview, atomic file operations if available, rename/delete rules, protected paths, durable journal and rollback. Do not write/promote results to a real repository.

D. Issue a reasoned choice: existing-root direct grant (not permitted under current invariant); staged synthetic root (candidate); dedicated-principal stage (C052-dependent); strict BaseContainer/guest (C053-dependent). If reasonable development tasks require access to original secrets, writes outside the stage, executable host ACL changes, or unrestricted IPC, close as negative. Note that host *reads* remain broad even if promotion is safe.

## Acceptance / stop

Qualifying outcome requires reproducible usable toolchain execution, positive allowed writes, negative outside writes, verified descendants and cleanup, no source descriptor changes, measured overhead, and a reviewable staging threat model. Staging promotion needs a **new accepted staging ADR plus C046** and independent concurrency/security qualification; do not expand C051 to ship it. Preserve C015–C018 blocked history. Any untested alias, read, IPC or network path remains unclaimed. No new public profiles, storage/schema/installer, production launcher or release. Run focused Windows native tests plus the same fmt/ADR/sandbox/ownership/verify guards as C050. Closure target records evidence, stop reason and an explicit C046 decision request if positive.
