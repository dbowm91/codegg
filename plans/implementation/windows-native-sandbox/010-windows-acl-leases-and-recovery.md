# Windows Native Sandbox Corrective C010 — Windows ACL Lease, Contention, and Crash Recovery

Status: active

Repository baseline: `17ef5e09b46fa364a520c8dc8793ab866020674f`, branch `work/windows-native-sandbox-foundation`, 2026-10-11.

Corrects original implementation plan: `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md`.

Original blocked closure evidence: `plans/closure/windows-native-sandbox/004-status.md` (immutable historical finding).

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`. No new architectural decision is authorized.

Primary class: infrastructure

## 1. Objective

Build an owner-verified, crash-recoverable ACL/profile lease system and compile bounded Windows filesystem grants for AppContainer restricted launches; prove strict restoration on disposable roots and validate prerequisites to full profile guarantees. Remain honest about absent privileged adversarial evidence.

## 2. Why this milestone is ready / dependency gate

Hard: C009 closes with an integrated, gated restricted process owned by `ManagedProcessService`; C008 pointer lifetime repaired. Hosted Windows Server 2025 can test leased ACL operations on newly created disposable directories, so no privileged VM is required for a bounded infrastructure pass.

## 3. Current implementation evidence and failed predecessor coverage

- M004's PowerShell fixture saves/restores security descriptors on temp directories but only in-process; it is **not** a crash-recoverable production lease.
- `SandboxLaunchSpec` carries read, write and denied paths, but Windows selected backend is absent. Under an ordinary token, broad ACEs, linked/sibling workspaces, hardlinks/junctions and OS runtime paths can break a naïve grant-only model.
- M004 closure records high-risk missing production ACL lease/rollback and no privileged VM.

This is a corrective of the **blocked M004**, not a claim that `004-status.md` closed or that Windows filesystem sandboxing works. Its security regression tests must capture the evidence that the previous pass failed to obtain.

## 4. Invariants that must not regress

- No persistent ACL mutation unless the user/installation owns the root and its before-image can be safely restored.
- Leases must be attributable to a specific run, principal, path identity and descriptor revision, never broadening unrelated roots.
- Crash/restart restoration must avoid deleting another administrator's changed ACEs; detected conflict quarantines/blocks further constrained launches.
- Capabilities remain unavailable until observed read/write guarantees and C011 evidence; no network claim.

## 5. Scope

### In scope

- Introduce immutable Windows path/volume identity and per-run ACL lease transaction state in Codegg daemon-protected storage.
- Use owned `AppContainerSid` grant ACEs for explicit read/toolchain/write roots, with sensitive denies never granted; compute least privilege for ReadOnly and WorkspaceWrite.
- Implement create/validate, journal-before-mutation, apply, deny on partial failure, release, recover after restart, and idempotent operator repair inspection on Codegg-owned paths.
- Hosted fixture coverage with interruption and ACL descriptor before/after equality on disposables.

### Explicitly out of scope

- Arbitrary system-drive/user profile re-ACL, broad Everyone/Users ACE modification, admin-only host provisioning by default, firewall, security guarantee certification without VM matrix, changing network isolation or model permission policy.

## 6. Required production changes

### Security policy compiler
Map requested read/write/deny roots to native descriptor operations and AppContainer SID; no blind grants under forbidden descendants. Scope grants and recurse policy precisely; enumerate which runtime/dynamic toolchain directories are read-only and which caches require temporary write. For existing directories, require owner/descriptor capture, volume/identity pinning, robust rejection of unresolved reparse/hardlink threats or explicit unsupported result.

### Durable lease and recovery
Use an atomically persisted protected record before ACL writes: root identity, original descriptor hash/bytes, owned ACE identity, exact change, expected after-image, process/Job generation and state. Commit states with crash-safe fsync as applicable. Startup recovery checks record and live descriptor before restoring owned bytes/ACEs; never overwrite concurrent third-party changes. Release only after Job terminated. Bind record to owner and workspace.

### Deployment
No privilege elevation from untrusted tools. If operator-owned existing roots cannot be safely granted, leave unsupported and return reason; surface bounded diagnostics and a manual repair path for stale owned state.

### Tests/docs
Prove identity-safe lease contention, restart/recovery, partial ACL failures, and exact before/after descriptor restoration on disposable runner directories.

## 7. Ordered work packages

### WP-A — Allowed root/deny policy
Produce deterministic Windows policy with explicit test roots, runtime reads and scratch; fail unknown paths and forbidden ancestry. Test sibling roots and dynamic libraries.

### WP-B — Journaled lease
Design protected durable state, create owner-checked grants, preserve before-images and handle incomplete transitions; hold SID/profile and Job lifetime until after cleanup.

### WP-C — Concurrency and crash recovery
Recover killed parent midway through mutation, kill after Job start, stale lease, reused path, simultaneous sibling worktrees and collision with operator ACL changes. Exact descriptor matching, conflict recorded; never broad ACL reset.

### WP-D — Test-only managed process
Run C009 restricted child against leased fixture roots; assert synthetic secret read denied, allowed write/read succeeds, outside write denied, and cleanup exact. Keep production backend disabled until C011's complete adversarial matrix.

### WP-E — Installation risk review
Document privilege requirements and unsupported existing workspace ACL scenarios; do not write to system ACLs or account data.

## 8. Failure, cancellation, restart, and contention semantics

Make ACL setup and rollback a finite state machine: prepared→journaled→applied→process-attached→released or quarantined. No untrusted child resumes before every lease is applied and validated. Cancellation/timeout terminate Job before release. Restart locks recovery per lease; if descriptor drift or identity mismatch, preserve evidence and block retry instead of overwriting. Two runs must not remove each other's SID ACE; bounded retries and conflicts are explicit. Durable record writes must resist torn updates and path replacement.

## 9. Compatibility and migration

Choose existing daemon-owned persistence where appropriate, or a small versioned protected lease journal if storage schema cannot cleanly represent transient filesystem state. No user-facing protocol break without an ADR. On upgrade, unknown/incomplete lease versions fail safe. No migration that adopts arbitrary preexisting ACL changes. Unix unaffected.

## 10. Required tests

- Hosted Windows: disposable root creation, ACL grant, permitted child read/write, denied read/write, descriptor before/after identity, cancellation cleanup.
- Host fault injection: terminate between journal/write, write/job start, release and journal deletion; startup reconciliation, stale SID, changed ACL conflict, two simultaneous worktrees.
- Security negative: inherited ACEs, parent path replacement, symbolic link/junction, hardlink, ADS, UNC, protected system path; unknown cases return unavailable, never success.
- VM-only privileged tests are **not required to close C010 infrastructure**, but must be explicitly carried forward to C011/M004 for real existing-path authority.

## 11. Required verification commands

On Windows hosted:

`cargo test --locked -p codegg --test windows_acl_lease -- --nocapture --test-threads=1` (new target)
`cargo test --locked -p codegg --test windows_restricted_managed_process -- --nocapture` (C009 target)
`cargo test --locked -p codegg --lib windows_policy_tests -- --nocapture`

Cross-host:

`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`cargo fmt --all -- --check`
`bash scripts/verify.sh quick`

The closure must record exact versions, run IDs, skipped coverage and failures. Do not mark tests passed if the hosted runner has no rights to execute them. New test filenames are prescribed as targets to create, not files assumed to exist today.

## 12. Documentation updates

`architecture/security.md`, `architecture/testing.md`, `docs/windows-acl-leases.md`, M004 corrective roadmap, registry, and C010 closure.

## 13. Acceptance criteria

- On disposable hosted roots, exact before-image ACL restoration succeeds after success, failure, cancellation and restart.
- Journal recovery never changes a path whose identity/descriptor has changed without deliberate repair; concurrent runs isolate their grants and cleanup.
- A test-only constrained child is denied synthetic secret and outside writes; no production Windows `filesystem=Enforced` claim yet.
- Every unsupported ordinary workspace/privilege case returns a typed unavailable reason.

## 14. Stop conditions

Stop if safe recovery requires global ACL grants, unseen host principal elevation, overwriting unrelated ACEs, no workable persisted lease authority, or any case where a failed grant leaves a broader active permission. Record a risk/architecture decision and keep Windows unsupported.

## 15. Closure evidence required

State machine and storage ownership docs, hosted native ACL before/after comparisons, fault/restart/run interleavings, conflict refusal, actual allowed/denied child traces, explicit unrun privileged VM tests, release-gate disposition.

Do not edit the historical `plans/closure/windows-native-sandbox/004-status.md` to turn its failures into successes. Create `plans/closure/windows-native-sandbox/010-status.md` and make a later M004 requalification/closure decision from aggregated evidence.

## 16. Handoff notes

The user currently has only a hosted Windows runner, so execute C010 on **runner-owned disposable directories**. Do not mark M004 closed based on that narrower fixture evidence. C011 owns the missing privileged/reparse/credential/IPC adversarial qualification.
