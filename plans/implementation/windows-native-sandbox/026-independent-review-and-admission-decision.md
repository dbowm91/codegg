# Windows Native Sandbox C026 — Independent Windows Security Review and Admission Disposition

Status: closed (no-admission; C024 did not qualify a restricted-token candidate)

Repository baseline: `4d61f9622ee037313ce4781eb5b5666142d95a5d` on `work/windows-native-sandbox-foundation`; refresh HEAD before implementation.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-corrective-qualification-after-c022`.

Long-term requirements: `plans/000-long-term-specification.md` sections 4, 17, 27 and 29; `plans/001-terminology-and-domain-model.md` section 8.

Applicable ADRs: ADR-0004 (approval/profile), ADR-0016 (sandbox launch authority), ADR-0019 (historical AppContainer experiment), ADR-0020 (feasibility only).

Primary class: security invariant / infrastructure.

## 1. Objective

Obtain an independent security assessment and make a documented **reject / insufficient evidence / qualified to propose new production ADR** disposition for Windows confinement after C024 and, if justified, C025. This plan itself grants no product admission.

## 2. Readiness

C022 `plans/closure/windows-native-sandbox/022-status.md` found **no strict-positive winner**. C024 written positive/negative/blocked disposition is mandatory. **If C024 is negative**, close C026 as no-admission immediately without C025. **If C024 positive**, additionally require C023 green CI, C025 strict lifecycle/recovery closure and a distinct security reviewer. Missing independent reviewer blocks any positive recommendation.

## 3. Current evidence

C019 shared child oracle closed; C020 MXC v1 Windows 11 ProcessContainer observed positive denials but failed Python/native helper initialization and lacked inherited-handle proof (bounded negative); C021 Windows Server separate **standard** account with private ACL host fixtures passed Git/Python/Cargo/IPC/handle checks; C022 conditionally closed insufficient evidence. Historical C011 host reads, C013 hardlink-descriptor mutations and C014 AppContainer Git failure remain real findings. C023 required CI currently blocked by closed-negative C014.

## 4. Invariants

Requested-versus-obtained read, write, Job and network guarantees must be separately evidenced. No original populated root ACL/MIC changes, privileged agent helpers, silence of denial tests, broad original user profile grants or FullHost fallback. Windows production `SandboxRequest::Required` stays unavailable; M004/M005/M006, C011 and C015–C018 do not unlock merely because C026 documents review.

## 5. Scope

Independent review of C024 token/group ACE and named resource proof, process setup/Job ownership, inherited handles, system/registry/IPC/UNC/device and DLL/profile exceptions; C025 receipts, credentials, crash/recovery, concurrent workspaces and cancellation; correct test/CI interpretation under C023. Non-goals: implement backend, approve privileged account setup, change schema, production UX, network guarantee or revive failed MXC run.

## 6. Expected changes

Documentation + specific reproducible reviewer findings only; possible **proposed** future production integration ADR if and only if strict-positive proof and independent review exist. That future ADR would explicitly sequence an identity foundation before M004, bounded staging, managed execution and approved output reconciliation; keep M005 firewall isolation independently gated. Never auto-revive AppContainer-specific C016 or bypass an independent C011 security requalification.

## 7. Work packages

A. Freeze evidence SHAs and native CI/VM run IDs/host OS editions for C019–C025; classify each claim as child-denied, granted, parent preflight, model, unrun or unavailable. Record all missing dimensions and exact threat boundaries.

B. Independent reviewer examines raw policy/token, DACL/group access checks, hardlink/ADS/reparse, process creation and Job-before-resume, handle/broker and IPC, executable/DLL search, prohibited host cache, privileges/credential exposure, concurrency/cleanup and network exceptions. At least one attack reproduction not authored solely by the implementing agent; document reviewer identity/independence.

C. If C024 positive, reproduce relevant C025 reboot/collision/cancellation and multi-workspace evidence, inspect original host descriptors and residual owned accounts. No claim for Windows 11 unless native host matrix actually run.

D. Issue one disposition: **negative candidate** (C024 failed), **blocked/insufficient** (missing host/recovery/review), or **feasible to propose distinct production ADR only**. Register findings by severity and next exact gate; no indefinite corrective loop.

## 8. Failure/restart/contention

Missing C024 strong read proof or unexpected original-host read means reject. Missing reviewer/reboot means blocked. Positive review may propose an ADR but not provision customer accounts or change status.

## 9. Compatibility

No public API/config/storage/migration, Windows release guarantee or Rust MSRV changes. MXC optional negative remains historical and may be separately reconsidered only under new architecture decision.

## 10. Tests

Review and independently reproduce representative broad-group-readable host probe, token SID restriction, direct/alias/handle/IPC and C025 recovery/collision matrices; report actual native selected counts and all unrun conditions. No permanent lint framework just for review.

## 11. Verification

`git diff --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `cargo fmt --all -- --check`; `bash scripts/verify.sh quick` where code changed. Preserve exact native run IDs; distinguish documentation review from actual Windows test.

## 12. Documentation

`plans/closure/windows-native-sandbox/026-status.md`, Windows roadmap/registry and a proposed production ADR **only** if proven. Do not rewrite accepted ADR-0019/0020 or previous closure records.

## 13. Acceptance

Signed-off scoped independent reviewer and a fully enumerated evidence matrix, with explicit **no-admission / blocked / candidate qualified for later ADR** verdict. Never claim production `Enforced` in this milestone.

## 14. Stop conditions

C024 leak/negative, missing C025 reboot, no independent reviewer, unresolved SID/job/handle/credential risk, or requested production shortcut. Report and stop without rolling an "almost closed" security test loop.

## 15. Closure

Exact review author/independence, SHAs/Windows builds, positive/negative/unrun trace evidence, findings severity, compatibility/recovery evidence, open risks and explicit prospective ADR decision.

## 16. Handoff

Review is a decision gate, not implementation. Subsequent product work requires separately accepted architecture and new bounded implementation plans.

## 17. Independent access-check review and disposition refinements

The reviewer must inspect **both** C024's positive normal-sandbox-account read baseline and the negative true-restricted-child read of the *same* unchanged broad-group-readable synthetic host object. A private-DACL-only denial, a parent-side `AccessCheck` result, a `WRITE_RESTRICTED` write denial, and a simulated/absent restriction are **not** acceptable read-confinement evidence. Independently sample the actual child `TokenUser` / restricted SID memberships, original object DACL/owner/control and per-alias file ID, stage access, named-pipe and inherited handle observations. The reviewer must state whether restrictions continue to hold after descendant creation and process restart.

Examine the privileged helper trust boundary separately: identity/credential storage, `CreateProcessAsUserW` privilege prerequisites, startup desktop/window-station and GUI messaging risk, daemon elevation exposure, resource ownership/reconciliation, Job-last-handle cleanup, and attempted child breakaway. A test fixture running as an administrator is **not** evidence that an ordinary user-mode daemon can safely perform account/token provisioning. Document the intended, separately authorized provisioning model before allowing even a **proposed** production ADR.

Independent test re-execution must use a different fixture or perturbation from the implementation author's primary test where feasible; report reviewer independence and raw native outcomes. Any inability to establish a safe broker/account lifecycle or to deny group-readable host files yields `no-admission` or `blocked`, not 'conditionally supported'. Windows network behavior remains unclaimed regardless of filesystem result.

Microsoft authority: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken ; https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw ; https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects .
