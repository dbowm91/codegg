# Windows Native Sandbox C028 — Token Launch Initialization and VM Privilege Diagnosis

Status: implemented (negative bounded hosted phase; privileged comparison remains VM-gated)
Primary class: infrastructure/invariant
Repository baseline: `7a27bc7e6566616fc1b2b8a69637f12050d42843` on `work/windows-native-sandbox-foundation` (2026-10-13).
Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0021-launch-authority-decision-gate`.
Architecture proposal: `plans/adrs/ADR-0023-windows-privileged-launch-vs-guest-isolation.md` (not accepted); research: `research/windows-native-sandbox/adr-0021-authority-options.md`.
Canonical references: `plans/000-long-term-specification.md` sections 4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` section 8; `plans/003-planning-process.md`; ADR-0004, ADR-0016, ADR-0020.
Immutable evidence: C024, C026 and C027 status records.

## 1. Objective

Isolate the 15-second unrestricted `CreateProcessWithTokenW` startup timeout and document the independent `CreateProcessAsUserW` privilege gate. Produce a bounded, native test-only explanation, not a new sandbox backend.

## 2. Readiness and dependencies

C027 negative closure is a sufficient predecessor, helper/PowerShell fixture exists. Hosted runner can execute nonprivileged diagnostics. The `CreateProcessAsUserW` privilege-enabled comparison **requires** a separately consented isolated disposable Windows VM; absent VM must be recorded blocked, never guessed.

## 3. Current repo evidence

`scripts/windows_restricted_primary_token_feasibility.ps1` suspends process, checks token/Job, calls `ResumeThread` but ignores its previous suspend count unless -1, waits for JSON output and kills Job at 15 seconds. No first-instruction marker, so phase of timeout is unknown. `CreateProcessWithTokenW` may run in caller session without loaded user HKU profile.

## 4. Security invariants

Constrained Windows `Required` unavailable; no original ACL change, host-wide privilege change, agent-elevated child, guessed read-denial or network claim. Verify child token/Job before resume, kill on failure and preserve C027 negative evidence.

## 5. In-scope / out-of-scope

Only test-only bounded Win32 trace/fixture in `scripts/windows_*` and optional Windows workflow. No broker, daemon runtime code, credential stores, real user paths, MXC retry or production service.

## 6. Required changes

Add `scripts/windows_token_initialization_trace.ps1` and a minimal native marker fixture, or narrowly extend C027 siblings. Capture `ResumeThread` previous suspend count, process state, session/HKU/profile, DLL/CRT loader/desktop, env/console, first-instruction and result-file events with timestamps; keep target account secrets out of logs.

## 7. Ordered work packages

WP-A: exact same-token baseline, flag/argv/host and thread suspension counts; first instruction vs JSON. WP-B: compare only `LOGON_WITH_PROFILE`, desktop, target env/stdio and no-CRT/loader variants with unchanged baseline. WP-C: isolated VM only, record scoped `SeIncreaseQuotaPrivilege`/`SeAssignPrimaryTokenPrivilege` and control `CreateProcessAsUserW` with operator consent. WP-D: classify suspend/loader/profile/session/output-path/privilege or unresolved and close.

## 8. Failure, cancellation, restart and contention

Owned Job kill-on-close, terminate-and-wait on timeout, no inherited privileged handles. Unique account SID/stage/marker paths; delete only owned resources. Absent VM means privileged probe unrun. No reboot/service recovery claims.

## 9. Compatibility and migration

No config/API/schema/MSRV/installer change. Windows Server 2025 results do not prove Windows 11. Unix behavior unchanged.

## 10. Required tests

Real executable first-instruction marker, previous suspension counts, process exit or suspended state, missing DLL/control, denied JSON write, token SID/Job and cleanup negative injections. Explicit privileged VM comparison if host eligible.

## 11. Verification commands

Windows: `pwsh -NoProfile -File scripts/windows_token_initialization_trace.ps1` (new script), existing `windows-dedicated-user-feasibility.yml` opt-in and `cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`. Cross-host `git diff --check`, `cargo fmt --all -- --check`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `bash scripts/verify.sh quick`. New fixture names are to be created; require nonzero executed tests.

## 12. Documentation updates

`research/windows-native-sandbox/c028-launch-trace.md`, VM runbook, tested privilege/phase matrix, C028 closure and registry/roadmap statuses.

## 13. Acceptance criteria

Observed suspended count and first-instruction/loader/result phases accurately distinguish hang mechanism or a precisely isolated unresolved case. Clean Job/account. VM control tested or explicitly operationally blocked. No restricted read-denial claim unless real restricted child actually runs.

## 14. Stop conditions

Stop after the finite comparisons if unresolved; no enabling privileges on shared runner, original-host ACL mutation, broad user-profile grants or agent-accessible privileged helper.

## 15. Closure evidence

Exact SHA, Windows build/UBR, native workflow, privilege table, process/session/suspend counts, first-instruction and failure phase, child exit and cleanup, observed test counts, positive/negative/blocked C029 inputs.

## 16. Handoff

C028 alone never unblocks C025/C026/M004. C029 owns architectural comparison; no automatic subsequent launch workaround.

Write `plans/closure/windows-native-sandbox/028-status.md` with exact native evidence or a blocked/negative disposition; never rewrite historical closures.
