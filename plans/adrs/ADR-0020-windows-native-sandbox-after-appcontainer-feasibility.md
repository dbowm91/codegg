# ADR-0020: Windows Native Sandbox After AppContainer Feasibility

Status: accepted (**bounded successor feasibility comparison only; production mechanism not selected**)

Date: 2026-10-12
Decision owners: project maintainers

Related specification sections:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Affected roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md`.
Related ADRs: `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md` and `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`.
Evidence: `plans/closure/windows-native-sandbox/011-status.md`, `013-status.md`, `014-status.md` and `research/windows-native-sandbox/adr-0020-harness-comparison.md`.

## Context and observations

C011 demonstrated direct/junction/hardlink reads of an explicitly denied synthetic host credential. C013 proved that modifying ACLs of populated original host trees can mutate hardlink-shared objects and leave descendant descriptor-control drift after rollback. ADR-0019 accepted *only* staged AppContainer feasibility. C014 then launched a real test-only child and read/wrote a disposable projection, but Git `init` failed opening `/dev/null` under the tested grants; alias, IPC, handle, Cargo and Python probes never executed. C014 is closed **negative**, and this does not prove all AppContainers are unusable nor that host-read denial works. C015–C018 are still blocked.

The previous M007 Microsoft MXC experiment was deferred on an older pinned revision with no eligible Windows 11 runner. Upstream MXC v1.0.0 (2026-10-07) and VS Code's experimental Windows agent integration warrant fresh qualification, **not** untested trust in vendor claims. Codex and Anthropic separately demonstrate dedicated sandbox-account patterns; their use of `WRITE_RESTRICTED` or additive original-host ACEs is not proof of Codegg-level denied *read* semantics.

## Decision drivers

1. A running child may be allowed to read/write only under a claimed and actually measured confinement mechanism; read and write guarantees are separate.
2. Never alter original populated workspace/profile/daemon IPC DACLs, SACLs or integrity labels, nor broad host-device descriptors, merely to make tooling run.
3. Preserve real Git/shell tool compatibility, controlled descendants, stable workspace/worktree authority and strict requested-versus-obtained reporting.
4. Prefer a Windows-native, non-VM path where feasible; permit explicitly consented provisioning in a disposable experiment without requiring all product users to elevate.
5. Evaluate host-version/edition, signed binaries, SDK/MSRV, administrator/credential lifecycle, persistent state, cleanup, latency and supported toolchains.
6. Avoid another verification loop: two bounded candidate spikes with explicit positive/negative/blocked results and a separate decision, not automatic fixes.

## Considered options

### A. Native Windows process isolation through MXC v1 / process-security environment

Candidate for C020. Pin exact MXC v1 SDK/binaries, check live host capability, selected backend/tier, requested-versus-honored rules and observed read/write/deny behavior. Validate Windows 11 serviced build and vendor release limitations. Reject an AppContainer+DACL fallback that relies on modifying original populated host objects. The stable v1 SDK interface alone does not demonstrate a qualified security boundary; preview policy warnings remain material. Avoid raising Codegg's Rust 1.89 MSRV for a spike.

### B. Separate low-privilege Windows local principal

Candidate for C021. Test administrator-consented disposable identity with restricted token and Job under existing ownership, small independent staging root and private user environment, informed by Codex/Anthropic Windows designs. A distinct SID and `WRITE_RESTRICTED` token are not read-confinement guarantees. Require observed host read denial through alternate paths and safe removal of every created identity, credential and ACE. No original source-tree ACE mutation.

### C. VM/Windows Sandbox/WSL2

Deferred independent backend. Stronger host boundary is possible but introduces virtualization/Windows-edition requirements and native-toolchain differences. Retain if both native candidates fail or for future stronger risk tiers. A WSL/Linux sandbox must not claim native Win32 toolchain coverage.

### D. Retain constrained Windows execution unavailable

Remains the production default and valid final decision if no candidate qualifies. Never silently fall back to unrestricted execution on denied/unavailable requests.

### E. Restart old AppContainer original-root ACL lease or projection pipeline

Rejected as a near-term implementation direction. C013/C014 historical failures remain. The `/dev/null` device hypothesis may be diagnosed in a disposable test VM, but must not justify global object security changes or automatically reopen C015.

## Decision and exact gates

1. **Accept only a bounded, test-only comparison:** C019 shared actual-child probe contract and C014 device diagnosis, followed by *independently closable* C020 (MXC v1 native feasibility) and C021 (dedicated-user feasibility), then C022 comparative disposition.
2. C019 is dependency-ready with existing Windows fixture infrastructure. C020 and C021 each hard-depend on C019, are parallel to each other, and have independent **operational** host gates (serviced Windows 11 for native MXC; consented disposable admin VM for identity). C022 depends on both written dispositions; it may close as *blocked/no candidate* when operational evidence is missing, but may not recommend production admission based on a blocked result.
3. **A positive candidate requires both:** at least Git + one native shell and representative scoped toolchain observation, **and** a real running-child deny matrix against original synthetic host data and IPC/handle aliases, with declared exceptions. No broad original-user credential/cache grants. Extra Python/Cargo capabilities require separate actual execution proof; missing them narrows support.
4. For each test report observed successes/failures as independent findings: do **not** short-circuit the adversarial matrix after toolchain failure, except where no child can be created. An unrun operation is “not run,” not denied.
5. C022 selects a *proposed integration route* based on evidence, with an explicit later maintainer production-admission decision. Only that later decision may register a production backend and re-sequence C015–C018 or dedicated-user/M005 foundations. C022 itself neither clears C011/M004 nor provides filesystem/network `Enforced`.
6. Any production successor reuses canonical `ManagedProcessService` authority; no parallel unmanaged command runner, privileged agent-shell setup, silent FullHost fallback, or misreporting Job-only isolation as filesystem enforcement.
7. Windows Server 2025 build 26100 is not automatically an eligible Windows 11 native-process-isolation host. Feature, OS edition, installed servicing, selected tier and exact dependency revisions must be measured on the host.
8. Original host workspace hardlinks, ADS, reparse aliases, NTFS object identity, stale promotion and pre-opened handles are mandatory adversarial classes; developer-tool read allowances require narrowly evaluated host-secret exposure.
9. Do not merge or declare unrelated branch work reviewed as part of this decision.

## Consequences

Positive: converts the C014 stop into finite experimental options; preserves closed-negative findings; creates comparable evidence rather than accumulating speculative platform fixes; can favor OS-managed native isolation if available and a dedicated principal if necessary.

Negative: may require a serviced Windows 11 VM, administrator-controlled disposable VM and extra test infrastructure; the two candidate paths may both fail or remain operationally blocked. Production Windows constrained execution stays unavailable.

Deferred: production snapshot/promotion engineering, dedicated-user global setup/credential rotation, persistent Windows firewall rules, alternate VM/WSL backend, Windows product UX/settings or protocol changes.

## Compatibility, migration and security/reliability

This ADR authorizes **documentation and disposable feasibility experiments only**, not production account creation, source ACL edits, executable backend registration, public API/schema changes, or distribution changes. Any elevated experiment is manual, on isolated disposable host with operator consent, reversible inventory and teardown verification. No network containment until separately evidenced with real sockets/descendants, WFP/firewall policy and bypass/loopback tests. Process creation is suspended until all mandatory checks and Job association succeed; on error, terminate/reap before deleting owned resources. Capture cancellation, reboots, crash/recovery, concurrent worktree isolation, time/size limits and before/after descriptor integrity.

## Verification and stop criteria

Evidence matrix in C019 applies identically to both candidates. Native true/false read/write, toolchain, descendant, IPC and leak probes, selected backend/tier, host permissions, cleanup and process supervision must have exact-SHA native output and pass/fail counts. Missing host produces blocked closure; positive/negative experiments are both legitimate closures. If denied resource reachable or unsafe persistent mutation required, close candidate negative. Do not use plain cwd copy, path normalization, runner status text or vendor marketing as a security proof.

## Supersession

Supersedes the **undecided successor choice** in this ADR's earlier proposed draft, not the historical accepted bounded ADR-0019 experiment, which ended at C014. ADR-0016, C011 and M004 security gates continue unchanged.
