# Windows Native Sandbox Corrective C018 — Native Projection Security Qualification and C011 Re-entry

Status: blocked (C017 strict positive closure plus independent Windows security fixture)

Repository baseline: `d00f1274c844bde7055559f240c020ffb2f1ff46` (`work/windows-native-sandbox-foundation`), 2026-10-12.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0019-projection-successor-milestones`

Accepted decision (experiment only): `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`.
Research basis: `research/windows-native-sandbox/adr-0019-successor-assessment.md`.
Inherited blocked security gate: `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md`; `plans/closure/windows-native-sandbox/011-status.md`; C013 `plans/closure/windows-native-sandbox/013-status.md`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Other applicable ADRs: `ADR-0004` approval/profile independence; `ADR-0016` platform-owned launches and truthful obtained guarantees.

Primary class: security qualification.

## 1. Objective

Qualify the staged-projection/AppContainer path end-to-end on native Windows, including operator/data controls, permitted input/output, real denied host accesses, lifecycle and restart. Then make a documented strict *re-entry decision* for C011/M004; this does not pre-approve a production sandbox backend.

## 2. Readiness and hard gates

Hard: C014–C017 strictly positive, accepted ADR-0019 experimental direction unchanged, actual Windows Server 2025 hosted tests and isolated disposable privileged Windows VM for the remaining C011 security matrix. Windows 11 support is only claimed after separate Windows 11 evidence. C011 must independently accept all requirements.

## 3. Repository evidence and security problem

Known negative C011/C013 evidence persists; previous hosted C012 tests demonstrate pre-spawn rejection of unsafe roots, not running-child denial. New projection may be functional yet retain bypasses via system grants, COM/registry, inherited handles, original worktree aliases, staging injection or output promotion. No security status based solely on successful CI job or a static policy manifest.

## 4. Invariants and boundaries

- Windows production `SandboxRequest::Required` remains **unavailable**; experiment/test-only runs must not return `SandboxExecutionOutcome::Enforced`, update public capability or select FullHost.
- Never mutate real source workspace ACLs, SACLs, integrity labels, original hardlinks, user profile permissions or daemon named-pipe DACLs. No elevation or new account without a separate accepted design.
- `ManagedProcessService` owns restricted process creation, token/Job before resume, all descendants, stdout/stderr and status; authenticated daemon retains authorization and workspace ownership.
- AppContainer package SID, runtime permissions and file projection are *obtained* access facts only when the native tests prove them. No network isolation claimed without separate actual tests.
- Inputs are untrusted when copied into the projection; outputs are untrusted after the child writes them. Neither is a direct authority to modify the host worktree.
- All file paths and resource ownership must be bound to a stable workspace/worktree/job identity; stale paths, unsupported metadata, security conflict or partial setup fail closed.

## 5. Production-code boundaries and intended modules

Implement native security test matrix and, **only after independent approval**, a production-admission *proposal* documenting the exact backend, allowed runtime roots, trust boundaries, filesystem read/write semantics, network status, OS versions and setup. Do not set `filesystem=Enforced` or register Windows backend as part of qualification without a distinct strict M004 acceptance/ADR update. Use canonical `ManagedProcessService` tool and scheduler paths in tests; no second executor.

No unrelated UI, editor, human shell, distributed daemon, branch-wide merge or protocol refactor is permitted in this plan. Add public protocol/DB fields only after recording an explicit compatibility decision; tests and private types are preferred at this stage.

## 6. Ordered work packages

### WP-A — Host-denial matrix
On real Windows, execute direct attempts to read/write synthetic nonprojected host credentials/roots via absolute paths, junctions, symlinks, hardlinks, ADS, UNC/volume/device namespaces, registry/COM and daemon named pipes. Verify child/grandchild under actual AppContainer token and Job; test inherited/preopened handles and environment/capability escape. Explicitly record any default-allowed system path and compare against contracted guarantees.

### WP-B — Staging adversary
Malicious snapshots with hardlinks/reparse/ADS, sensitive content inside allowed tree, renamed files during hashing, alternate stream injection, stage permission tampering, undeclared package capabilities, wrong worktree identity and concurrent sibling projection attempts. Invalid inputs must fail before child starts.

### WP-C — Promotion adversary
Output diff injection, direct host path, reparse/symlink/hardlink/ADS swap after preview, cross-workspace writes, stale dirty original, selective approval, cancellation and crash in mid-promotion; assert no unauthorized host write, no silent data loss and exact/owned recovery.

### WP-D — Compatibility/operations
Windows Git, PowerShell/cmd, scoped Rust/Python subset with minimum toolchain grants; package/subagent/job routes, output/exit/timeout cancellation, restart cleanup, startup p50/p95 and disk budget. Run trusted DLL/toolchain dependency analysis, distributed worktree owner tests and unsupported-host negative cases. Preserve Linux/macOS regression results.

### WP-E — Disposition
Obtain independent security review and strict trace matrix against original C011 `and` M004 acceptance. If every gate passes, register new explicit production-enablement implementation plan and the additively qualified C011/M004 closure decision as appropriate; otherwise keep production unavailable with precise failing tests. M005/M006 dependency status changes only after M004 strict closure.

## 7. Acceptance criteria

- Entire claimed read/write profile enforced by actual Windows kernel rights and materialization/promotion controls, not only preflight refusals.
- All source original workspaces and sensitive files remain ACL/descriptor unchanged; no unintended host writes or leaks, including links/ADS/IPC/handles.
- Output promotion never bypasses authorization or changes a dirty/conflicted path.
- Native hosted and privileged isolated VM tests recorded at exact SHA, with known OS/library/network limitations and zero unresolved high/medium security findings.
- Independent C011 review accepts full evidence or explicitly records a blocked/negative outcome. No false version parity or network isolation claim.

## 8. Failure, cancellation, recovery, concurrency and stop conditions

Stop on any running restricted child that can access an original denied resource, a promotion that writes an unapproved destination, inherited privileged handle, unresolved file alias, journal leak, failed crash cleanup, or toolchain host grant broader than approved. A negative native test supersedes a green unit suite; keep `Required` unavailable. Do not repurpose prior C010 test-only DACL leases or automatically fallback to native FullHost. Dedicated identity requires separate revisited architecture approval.

## 9. Verification requirements

Windows hosted exact-SHA `windows_projection_feasibility`, `windows_projection_snapshot`, `windows_projection_execution`, `windows_projection_promotion`, new `windows_projection_adversarial` target and `windows_sandbox_adversarial` original failure regression. Isolated privileged VM: daemon named pipe impersonation, inherited handles, descendants, restart/force-stop, ACL descriptor invariance, actual app/toolchain paths and Windows 11 only if claiming support. Unix sandbox containment, policy/ownership guards and `verify.sh quick`. Security reviewer signs matrix, no fabricated/skipped test acceptance.

All closure records must include exact SHA, runner/OS build, Rust version, workflow job link, **selected nonzero test counts**, actual child stdout/denied operation results, skipped native/privileged coverage and known regressions. A preflight refusal, compilation-only test, model statement or parent-generated status frame never substitutes for an actual running restricted-child security assertion.

## 10. Documentation and migration

Document supported OS builds/filesystems/toolchains, requested vs obtained filesystem/network dimensions, known AppContainer default grants, per-run staging limits, data freshness, post-process promotion UX/audit and rollback. New public options or production status require independent architectural decision and migration/compatibility plan; no branch-wide unrelated review is part of C018.

## 11. Closure record and handoff disposition

C018 strict positive can recommend reopening C011 for final security acceptance and writing an M004 production-admission plan; C018 negative closes its own research line without changing historical blocked states. No automatic M005/M006 unblocking.

Write `plans/closure/windows-native-sandbox/018-status.md` with strict positive or negative evidence, separate from historical `004-status.md`, `011-status.md`, `012-status.md`, `013-status.md`. Update the registry and roadmap only on verified results. A negative feasibility result is a valid *closed-negative* plan, not a reason to mark an unsupported production feature ready.
