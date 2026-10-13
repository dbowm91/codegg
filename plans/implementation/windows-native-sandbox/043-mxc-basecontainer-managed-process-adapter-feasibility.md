# Windows Native Sandbox C043 — MXC BaseContainer managed-process adapter qualification

Status: blocked — C042 contract satisfied at `edde2bebe`, operational eligible-Windows-11 host gate outstanding (closure `plans/closure/windows-native-sandbox/043-status.md`)
Class: infrastructure (test-only candidate)
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0027 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0027 (bounded test-only), proposed ADR-0028 is NOT product authorization.
Dependencies: C042 hard; ADR-0027 bounded test-only authority; pinned supported Windows 11 x64/ARM64 operational
Closure target: plans/closure/windows-native-sandbox/043-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective and historic blockers

C033 validated a bounded MXC read-denial/toolchain subset; C035 native child entry; C036 failed child-correlated capture and timeout cleanup; C037 lacked descendant handle proof; C038 selected no candidate. This plan is a **new managed-process adapter experiment with explicit lifecycle and policy contracts**, not a continuation of an open-ended diagnostic replay or authorization to turn on Windows SandboxRequest::Required.

## Work packages

A. Freeze source/SDK/runtime, license, feature support, security-tier selection, exact OS edition/build/UBR/arch and compiler/MSRV. Keep the Rust 1.89 production workspace unaffected (isolated helper with pinned toolchain if necessary). Validate actual schema and runtime behavior of fallback.allowDaclMutation=false or equivalent explicit original-DACL-mutation prohibition; inspect selected tier and fail if it silently uses AppContainer/DACL fallback. A host lacking native BaseContainer deny-path fidelity closes operationally blocked.
B. Build test-only PlatformSandboxLauncher-compatible adapter behind the canonical ManagedProcessService or a test-only interface exercise of that exact contract, not a parallel unmanaged production runner. Immutable executable/argv/cwd/env/stdio/provenance, private status, no arbitrary inherited handles, bounded input/output, Job ownership and pre-child policy verification. Strictly separate external SDK side process from privileged daemon authority; attest actual process/token, not self-reported stdout.
C. Verify native child first instruction, positive same-file broad read in uncontained control, negative direct/junction/case/hardlink/ADS/extended path and relevant UNC/device read, outside write/create/move/delete, allowed worktree/stage writes, Git init/status, cmd/PowerShell, native Rust/C helper, Python script and offline Cargo. Mark individual not-run operations honestly. No source/worktree DACL/SACL/MIC/owner mutation.
D. Reconcile SDK native subprocess/handle transfer contract: stdio, ordinary inherit controls, DuplicateHandle/cross-process privileges, process/token handles, named pipes and descendant IPC. Unsupported explicit transfer can be excluded only from an explicitly narrower advertised contract after independent review; unsupported or unknown operations cannot be claimed denied. Test timeout-child exit via OS process query, cancellation, close and crash recovery, repeated and concurrent worktrees, no breakaway. Distinguish denied operation from denial-event telemetry; separately test activity capture if advertised.
E. Emit JSON evidence with raw child/parent/Job IDs (synthetic fixtures only), exact SHA, SDK/host metadata, observed denial operations, unsupported/error classes, toolchain and process cleanup. One planned controlled correction allowed for a specific root cause; then close positive, negative or blocked without another verification loop.

## Acceptance / stop

Strict-positive **test-only** only if no forbidden host read/write succeeds, the supported effective BaseContainer tier and no-host-mutation guarantee are proven, minimum developer tools run, Job/descendant/cancellation cleanup is confirmed, and applicable handle/IPC channels are explained with direct positive/negative proofs. Workflow green alone insufficient. A strict-positive close enables independent review planning only, not production admission; any weaker result remains no candidate. No network=Offline without separately qualified WFP/MXC network enforcement and no original ACL grants or public backend registration.
