# Windows Native Sandbox C025 — Dedicated-Principal Lifecycle, Recovery and Concurrency

Status: closed without execution (C024 strict-positive hard gate failed; lifecycle candidate abandoned)

Repository baseline: `4d61f9622ee037313ce4781eb5b5666142d95a5d` on `work/windows-native-sandbox-foundation`; refresh HEAD before implementation.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-corrective-qualification-after-c022`.

Long-term requirements: `plans/000-long-term-specification.md` sections 4, 17, 27 and 29; `plans/001-terminology-and-domain-model.md` section 8.

Applicable ADRs: ADR-0004 (approval/profile), ADR-0016 (sandbox launch authority), ADR-0019 (historical AppContainer experiment), ADR-0020 (feasibility only).

Primary class: security invariant / infrastructure.

## 1. Objective

Only after a qualified denied-read token from C024, prove safe disposable-principal ownership and process/Job cleanup under provisioning failure, supervisor crash, reboot, cancellation races, account-name collisions and concurrent workspaces. This is still test-only feasibility, not installed account management.

## 2. Hard gates

C024 **strict-positive** denied-read result, not private-ACL or `WRITE_RESTRICTED` write-only result; C023 truly green launcher CI; consented disposable elevated **reboot-capable** Windows VM. If C024 negative, abandon this plan. If VM cannot restart safely, record blocked, not positive.

## 3. Current gap

C021 [38334807679](https://github.com/dbowm91/codegg/actions/runs/38334807679) created and removed one standard local user, killed a Job descendant and preserved synthetic fixture DACLs. No abrupt supervisor death, interrupted `New-LocalUser`/stage grant, persistent ownership record, orphan/restart, stale SID reuse, two simultaneous worktrees or cancellation race was measured. Script `finally` cleanup is not crash recovery evidence.

## 4. Invariants

Daemon/`ManagedProcessService` authority remains unique; no agent-controlled admin privilege or second scheduler. Stable per-run identity and manifest/receipt must prove account/SID/stage ownership; name prefix alone cannot authorize deletion. No original host ACL/SACL/MIC changes, no plaintext password in source/logs/argv, no extra network/offline or filesystem `Enforced` claim. Job assignment must precede resume, and a canceled run may never kill another workspace.

## 5. Scope

Test-only setup/run/reconcile/remove state machine, private receipt, isolated accounts and Job supervision, failure injection and reboot recovery. Non-goals: production account installer, firewall/WFP, profile cache grants, output promotion, schema/protocol/CLI admission or accepting a production backend.

## 6. Expected implementation

Separate elevated setup/recovery from restricted child launch, keyed by opaque owned-run ID + account SID + machine/VM instance + stage identity. Atomic or journaled states `Absent -> Provisioning -> Ready -> Running -> Cleaning -> Removed / Quarantined`. Reconcile only objects whose verifiable ownership matches; never delete foreign accounts including name collisions. Store credentials only via accepted Windows secret mechanism if persistence necessary; prefer test-only ephemeral protected material. Reuse C024 restricted launch; do not switch to unrestricted C021 convenience.

## 7. Work packages

A. Enumerate each resource creation/change and transaction boundary; implement idempotent test-only setup and removal receipt with complete rollback/fail-closed behavior.

B. Run **two simultaneous** worktree/jobs and distinct account/lease identities; prove cross-workspace stage read/write, named pipe and handle denial and independent termination.

C. Inject failures before/after user create, password/token creation, stage ACL setup, suspended spawn, Job assignment, descendant spawn, cleanup; kill broker/process and **reboot disposable VM** mid-run. Recovery must distinguish owned/orphaned/foreign resources and quarantine ambiguity.

D. Inject stale SID and same-prefix preexisting account, duplicated cleanup, missing secret material, late cancellation and restart races. Verify original fixtures/security hashes remain unchanged and no live descendant survives abandoned lease.

E. Measure setup/teardown time, p50/p95 (state sample count) and account/disk residue; document Windows edition/UBR/installation constraints and toolchain compatibility under C024's chosen restriction.

## 8. Failure/ownership/concurrency

Every incomplete provisioning state must be recoverable without a dangerous guess. Reaper terminates Job before stage deletion; no `CREATE_BREAKAWAY_FROM_JOB`; failed cleanup leaves a visible quarantined record and disables new sandbox execution. Late cancel cannot target a new generation. Host VM reset is a required observable, not parent-simulated `finally` cleanup.

## 9. Compatibility

No product API, migration, config or MSRV change. Windows Server 2025 is not proof Windows 11 customer compatibility. No permanent account provisioning on user hosts.

## 10. Tests

Two cross-workspace child matrices, process crash at every setup boundary, actual reboot + startup reconciliation, SID/name collision, simultaneous cancel/restart, inherited handles/pipes, Job child/grandchild termination, password missing/rotation handling, original metadata/hashes before/after and Git/shell tool compatibility under C024 token. Counts per test must be nonzero.

## 11. Verification

`git diff --check`; `cargo fmt --all -- --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`. Run C023 fixed required launcher CI and C024 strict-positive selected read fixtures at exact SHA plus new actual reboot-capable VM commands documented in closure. Do not invent a currently existing recovery test binary.

## 12. Documentation

`architecture/testing.md`, `research/windows-native-sandbox/adr-0020-harness-comparison.md`, roadmap/registry on observed result, `plans/closure/windows-native-sandbox/025-status.md`.

## 13. Acceptance

Actual crash/reboot + successful owner-safe recovery, no orphaned owned accounts or cross-workspace privilege, no wrong-principal deletion, no original host mutation, passing C024 strong-read boundary during concurrent sessions and valid process/Job cancellation. A green ordinary hosted account test cannot substitute for VM reboot.

## 14. Stop conditions

C024 not strict-positive; no safe ownership receipt; foreign account collision; no restart-capable VM; credential leakage; any descendant or cross-stage escape. Stop and report negative/blocked, not automatic production code.

## 15. Closure

Exact SHA, VM OS/build/KB, setup/run/restart commands and actual fault-injection matrix with selected counts, ownership state before/after reboot, account/Job cleanup and security descriptor hashes, resource cost and independently reproducible known limits.

## 16. Handoff

C026 consumes this as feasibility/recovery evidence. No `Required` or account installer may be enabled here.

## 17. Restricted-token authority across provisioning and recovery

C025 is reachable **only** after C024 passes the two-process broad-readable-file contrast: ordinary sandbox account reads the synthetic `Users`/`Authenticated Users`/`Everyone` allowed fixture, and its true read-restricted primary-token child cannot read that **same unchanged file** while executing the supported tool subset.

The owned-state journal must record (or securely bind) the account SID, limiting/restricting SID identifier, created stage object IDs, Job/run generation, authority/lease identifier and original fixture hashes without persisting credentials in clear text. A reconciler must not infer authority to delete an account from name prefix, current username or an unverified journal row; compare live SID and recorded resource ownership before every destructive teardown. Treat password/profile material, desktop/window-station permissions, startup environment and inherited handles as independently reviewable resources.

Failure injection must include a crash **after token construction but before suspended launch**, a crash after Job association but before resume, loss of the last Job handle, and restart with re-used account name but different SID. Reboot and orphan cleanup must verify the same restricted-token provenance rather than relaunching through the unrestricted C021 `CreateProcessWithLogonW` path. Missing privilege, stale SID, other-owned account, descriptor drift, or unknown Job handle ownership => quarantine/deny, never auto-recreate a broad-privilege child.

Job child/grandchild inheritance needs a direct attempted `CREATE_BREAKAWAY_FROM_JOB` negative probe. Job close/parent-death must terminate descendants even if they create additional processes, and a sandbox child must never inherit the privileged Job owner handle.

Official Windows references: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw and https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects .
