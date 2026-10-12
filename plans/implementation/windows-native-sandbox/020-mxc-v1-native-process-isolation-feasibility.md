# Windows Native Sandbox C020 — MXC v1 Native Windows Process Isolation Requalification

Status: blocked (hard: C019 accepted shared matrix; operational: supported serviced Windows 11 test host)
Date: 2026-10-12
Primary class: infrastructure / security feasibility
Objective: Reassess MXC v1.0.0 under the actual Windows native process isolation tier, qualify Codegg's read/write/deny contract and developer tool compatibility, and publish a bounded positive/negative/blocked result.

**Authority:** ADR-0016 (cross-platform launch authority), ADR-0019 (historical experimental record), ADR-0020 (accepted feasibility comparison), `plans/subsystems/windows-native-sandbox-roadmap.md`, and `plans/003-planning-process.md`.

**Baseline:** `b1024ba719c627f8a294cb50170d48ec08ce62f4` on `work/windows-native-sandbox-foundation` (2026-10-12), refresh HEAD before implementation.

**Security invariant for every plan:** Production Windows `SandboxRequest::Required` stays unavailable; no production backend registration, no `SandboxExecutionOutcome::Enforced`, no silent FullHost downgrade, no network confinement assertion without packet/socket evidence. Existing Linux/macOS backends, canonical `ManagedProcessService` ownership, identity/worktree ceilings and approval/audit remain unchanged. Fixture-only behavior is not release behavior. Preserve unrelated branch changes and keep Windows sandbox scope independent from the other diff.

**Closure convention:** report exact code SHA, Windows edition/build/installed KB, host privilege, target tool versions, child token/identity/backend, selected nonzero test count, logs proving *running-child* allowed and prohibited operations, skipped tests and reasons, provenance of isolated VM evidence, and before/after host security descriptors where applicable. A missing native/privileged host is **blocked**, an observed incompatibility or privilege escape is **closed negative**, and a passing compile or preflight is never positive security evidence. Close with `plans/closure/windows-native-sandbox/020-status.md`; update roadmap and registry on actual evidence only.


Long-term references: `plans/000-long-term-specification.md` ``4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` `8.

## Inputs and why the old M007 record is not overwritten

M007/`plans/closure/windows-native-sandbox/007-status.md` pins early MXC `c6f301d...` and is deferred without actual Windows 11 test evidence. This is **new** research, not a claim that M007 passed. Upstream MXC SDK `v1.0.0` published 2026-10-07; tag `d119563ff7fa77c990c1b3762693d03dc763b605` points to commit `7bf210247986cb73b1b314df60c2f1109c479c0b`. The documented Rust SDK public entrypoint is `mxc_sdk::v1`. MXC repository toolchain `1.93` differs from Codegg MSRV `1.89`; use an **isolated experiment toolchain or separately versioned signed runner**, no default dependency or MSRV increase.

Microsoft lists serviced Windows 11 process-isolation floors and capabilities in `docs/backends/process-container/os-version-support.md`; at research time 24H2 `26100.9278`, 25H2 `26200.9278`, 26H2 `26300.9550`, 26H1 `28000.2804`. VS Code's Windows agent documentation independently requires an applicable September security update and warns Windows is experimental. Treat documentation as host eligibility hypothesis, **not** proof that the requested policy is actually honored. MXC README continues to warn some generated policies are overly permissive.

Sources: https://github.com/microsoft/mxc/releases/tag/v1.0.0 ; https://github.com/microsoft/mxc/blob/v1.0.0/docs/backends/process-container/os-version-support.md ; https://github.com/microsoft/mxc/blob/v1.0.0/docs/host-prep.md ; https://github.com/microsoft/vscode/blob/main/src/vs/platform/sandbox/common/terminalSandboxMxcRuntime.ts ; https://code.visualstudio.com/docs/agents/run/agent-sandboxing .

## Authority and production boundaries

Use a stand-alone `research/windows-native-sandbox/mxc-v1` experiment or Windows test-only fixture. No MXC link/runtime dependency in the shipping Codegg process, no `BACKENDS` registry mutation, no automatic UAC, no SDK-driven broad host ACL/device changes. The parent `ManagedProcessService` remains authoritative in any future production integration; if MXC owns child lifetime in this probe, instrument equivalent kill/reap semantics and report the ownership difference for C022. Original source workspace paths/metadata may not be modified.

## Ordered work packages

1. **Pin and capability discovery:** freeze MXC release binary/sdk checksums and exact revision, OS edition/build/UBR/hotfix and Windows Feature/`processmodel.dll` contract; explicitly query selected MXC backend, tier and policy honor. Distinguish native process security environment from AppContainer/DACL fallback and unavailable. A `26100` Windows Server CI machine is *not* Windows 11 native-security evidence.
2. **Policy mapping contract:** document mapping of `SandboxLaunchSpec` read/write/deny (including denied descendants/aliases), temp, system executables, network, IPC and Job guarantees into MXC v1 policy; classify honored/unavailable/unsupported. No missing deny rule becomes default allow, and no silently broadened root. Analyze whether native isolated access can enforce *existing* objects without host-DACL mutation; capture actual before/after ACL/control hashes.
3. **Real runnable tool:** on a supported isolated serviced Windows 11 host, launch Git init/status and a PowerShell/cmd command under the requested candidate boundary; probe minimal Python and Cargo separately. Trace denied device (incl. null), DLL/registry/toolchain dependencies, without granting user credentials/caches.
4. **Run C019 adversary:** child itself tests direct secret, sibling, original workspace, junction, hardlink, ADS, IPC/pipe, inherited handle, descendant, cancellation and forbidden writes. Every claimed denial must result from an actual OS boundary; report known system read exceptions. If Git fails, *still* run negative matrix when child startup is possible.
5. **Lifecycle and operations:** repeated cold/warm launches (record sample sizes, p50/p95), disk/binary cost, per-run cleanup, two simultaneous workspaces, uninstall/host-prep rollback, source tree descriptor invariance; detect crash residue. Optional network packet matrix is informational; no `network=Enforced` claim.
6. **Close:** publish exact reproducible commands, selected test counts, logs and a capability matrix. Classify strictly positive, closed-negative or blocked; do not auto-activate or change C015–C018.

## Strict gate / stop

Strict positive requires native supported tier actually selected, **no permanent host/source security mutation**, independently observed allowed Git+shell and denied original-secret reads and denied outside writes (including aliases), controlled descendants and cleanup. A compile-only result, vendor tier name or test-level mock is insufficient. Stop negative if fallback is only AppContainer+DACL leasing of original populated host trees, host device ACL changes are required for nominal workloads without safe per-run isolation, read-deny cannot be enforced, or child/descendant escapes. Host feature unavailability is **blocked** with exact Windows/KB inventory, not a negative security test. No unbounded retries around an unstable vendor SDK.

## Verification and evidence

New isolated `windows_mxc_v1_feasibility` harness (or standalone documented runner if SDK MSRV differs), actual supported Windows 11 MSVC+SDK commands and C019 shared running-child fixture (not substituted by old Windows Server CI). Baseline local `cargo fmt --all -- --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`; `git diff --check`. Non-Windows cross-check cannot close native claims. Record signing/checksums, release API changes, permission/host-prep diff and cleanup. No Codegg public config, storage, protocol, installer or production backend change.

## Closure

Write `plans/closure/windows-native-sandbox/020-status.md`. Either a positive candidate or closed-negative candidate is sufficient to continue **comparison**, not production. Blocked operational evidence cannot be interpreted as success.
