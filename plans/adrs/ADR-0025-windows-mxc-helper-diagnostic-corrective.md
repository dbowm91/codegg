# ADR-0025 — Bounded MXC Native-helper and Handle-boundary Evidence Recovery

Status: **accepted — test-only C035 diagnostic authorization, not production candidate admission**
Date: 2026-10-13
Decision owners: Codegg maintainers
Planning baseline: `80139df351e9a77f46f7b09f904a0d6e854939b4` on `work/windows-native-sandbox-foundation` (verify current HEAD at implementation).

References: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`; ADR-0004, ADR-0016, ADR-0019, ADR-0020, ADR-0023 and ADR-0024; `plans/subsystems/windows-native-sandbox-roadmap.md`.

## Context and previous decisions

C033's exact Windows 11 ARM64 run `38404451372` at SHA `ff0443ca` used MXC SDK v1.0.0 ProcessContainer/BaseContainer and block-mode denial capture. It observed direct/junction/hardlink/case/ADS/extended-path/sibling read denials, outside write and synthetic pipe denial, functional Git init/status, cmd/PowerShell, a Python descendant and offline Cargo smoke. The normal child positively read the same broadly readable synthetic host object; 13 denial events were captured. C033 nonetheless closed negative: a separately launched native helper returned NTSTATUS `0xC0000142` (`STATUS_DLL_INIT_FAILED`), so an actual inherited-handle read probe did not execute. Its stage-write command was not independently checked, and child identity/group output was not obtained. The current harness also directs main and helper invocations to the same denial-capture output location, risking result/provenance conflation. The initializer cause is **unknown**; neither MXC nor a missing DLL is proved to be at fault by the status code alone.

C034's closure (`plans/closure/windows-native-sandbox/034-status.md`) explicitly admits **no production candidate** and says future evidence work needs a new explicit decision. ADR-0024 only approved bounded C032/C033 and the completed C034 comparison. ADR-0023 separately rejected the elevated SCM broker and native guest; C030/C031 remain terminally blocked.

## Decision

Authorize **only C035**, one bounded *test-only* MXC evidence-recovery corrective, whose authority expires at its own closure:

1. Isolate the C033 native helper initialization failure with a minimal ARM64 Win32 probe (including a first-instruction marker), one ordinary uncontained control, and a controlled matrix against the pinned MXC tier. Compare native probe in original location and independently staged location. Determine the earliest observable phase and record loader/architecture/import/environment/deny-event evidence without guessing a cause.
2. Produce a meaningful inherited-file-handle comparison: a **verified working uncontained inheritance control** against a synthetic, broadly readable file versus the actual MXC child. A child that never runs, a synthetic inherited handle never confirmed transferable, or a missing result file cannot count as containment.
3. Independently assert a fresh child-created stage-write file with nonce/content and record structured child token SID/groups/integrity + process provenance, or mark these unavailable.
4. Separate block-mode MXC denial telemetry *per invocation* and correlate reports to process identity, tier, run nonce and exit/timeout; retain C033 exact historical report unchanged.
5. Record a strict bounded positive, negative, or blocked disposition. **Even a positive C035 closure may only inform a later separately approved production-security ADR and implementation plans**, never immediately reopen M004/C011.

C035 may use a separate test-only GitHub Actions workflow on a qualified Windows 11 host or extend a clearly isolated opt-in MXC fixture. It MUST NOT register MXC in `src/security/sandbox/backend.rs`, modify production `ManagedProcessService` behavior, change Windows `SandboxRequest::Required`, emit filesystem/network `Enforced`, add installer/accounts/services/host ACL changes, silently fallback to FullHost, or mix its tests into required product support claims.

## Invariants and non-goals

- Host-original user workspace, profile, populated directories, credentials, source hardlinks, ACL/SACL/owner/MIC and existing system policy remain untouched; all probe files are synthetic and owned by the disposable test harness.
- Tests distinguish **actual executable entry** from API-returned success, a denied read from a failed launch, **write confinement** from **strict read confinement**, and evidence-collection CI success from sandbox qualification.
- The uncontained handle-transfer positive control must be meaningful, based on the actual Win32 inheritance contract. A deny outcome requires a running child that probed the specified handle and recorded the actual result; if the MXC SDK does not support testing the contemplated inheritance topology, mark that dimension unavailable.
- Do not expand `readonly_paths` to entire user profiles/caches merely to start a helper, disable read policy, switch capture to permissive mode to claim security, or patch host ACLs. Explicit runtime dependency needs must be inspected and risk-classified.
- Continue pinning MXC v1.0.0 and the isolated Rust toolchain unless a *separately recorded* version change is essential for a diagnostic; do not affect Codegg MSRV or product dependencies. Windows 11 ARM64 findings remain scoped to the tested build/tier, not Windows Server or Windows Home.
- Network, reboot/crash recovery, simultaneous worktrees, durable account credentials and general product launch authority are **not** qualified by this plan.
- All old records C020, C032, C033 and C034 remain immutable; ADR-0023 rejected/no-admission remains unchanged. No independent positive security review or production admission is asserted by this document.

## Alternatives and boundaries

Simply copying the existing MXC harness executable to `stage/handle-probe.exe` and rerunning the same command cannot distinguish toolchain import/CRT/architecture/bootstrap behavior from sandbox policy. A tiny separately built native helper with an explicit entry marker plus executable provenance and a no-sandbox control is the chosen diagnostic. A permissive audit mode could explain some synthetic launch dependencies but cannot qualify a confinement boundary; block-mode actual child results remain mandatory. The two-stage C032 topology is not reopened here.

## Readiness, closure and follow-up gate

C035 is dependency-ready as *test-only implementation* from C033's exact negative closure, C019's common native oracle and this accepted narrow ADR; executing the native suite additionally requires an eligible serviced Windows 11 ARM64 host and pinned dependencies. On an unavailable host, close blocked rather than claiming security.

Closure evidence must include: first-entry and exit results for controls/MXC; source binary digests/architecture/imports; validated uncontained inherited-handle exposure control and exact MXC child observation (or explicit unavailable); independently checked stage write; structured child token provenance (or explicit unavailable); distinct per-run denial telemetry; unchanged original host metadata; orphan-free process cleanup; and exact-SHA workflow links. Any hostile host-file read, leaked inherited secret handle, weakened security policy or unsafe host modification is a security-negative stop.

If C035 becomes strict-positive, a *new* production-integration/admission decision with independent Windows security review, lifecycle, host matrix, cancellation/concurrency and WFP/network qualifications remains necessary. C011/M004–M006/C015–C018 stay blocked; C030/C031 stay terminally blocked.

## Sources

- C033: `plans/closure/windows-native-sandbox/033-status.md`; `research/windows-native-sandbox/mxc-c033-harness/src/main.rs`; workflow `.github/workflows/windows-mxc-c033-requalification.yml`.
- C034: `plans/closure/windows-native-sandbox/034-status.md` and `research/windows-native-sandbox/c034-independent-comparison.md`.
- Microsoft Win32 process/handle access: https://learn.microsoft.com/en-us/windows/win32/procthread/inheritance .
- MXC SDK: https://github.com/microsoft/mxc .
