# Windows Native Sandbox Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`

Related ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`
- `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md` (accepted: bounded feasibility direction only; production not approved)
- `plans/adrs/ADR-0020-windows-native-sandbox-after-appcontainer-feasibility.md` (accepted: bounded C019–C022 candidate comparison only; no production backend)

Repository research baseline: `8e9d8b01e5c229715c8e4dea929e050b391e252c` (2026-10-10). This roadmap does not claim Windows execution or native security tests have already been qualified.

## 1. Purpose and ownership boundary

Deliver native Windows sandboxed **local process tool** execution through Codegg's authoritative `ManagedProcessService` and approval/sandbox policy. The security module owns backend policy compilation and obtained enforcement; the managed-process service owns actual spawn, handles, cancellation and outputs; the daemon retains authorization; Windows provisioning owns OS-specific identity/ACL/firewall configuration. No frontend, model or plug-in gains direct process authority.

## 2. Work classification

### Invariants
- Explicit profiles and parent/subagent ceilings cannot be broadened by choosing an easier backend; missing containment does not imply FullHost.
- No untrusted target starts before mandatory containment and process supervision succeed.
- Filesystem read/write, network and process/job facts are reported separately.
- Linux Landlock and macOS Seatbelt guarantees, existing Unix helper channel and signed/audited approval remain intact.

### Capabilities
- Native Windows `ReadOnly` and `WorkspaceWrite` file access only when objectively proven.
- Native Windows shell/structured command execution.
- Optional administrator-provisioned offline Windows identity with OS-enforced egress restrictions.

### Infrastructure
- Windows launch adapter, restricted token + Job Object ownership, private status channel, path/ACL handling, native CI and adversarial fixtures.

### Polish
- Operator diagnostics, security documentation, installed-helper packaging, and optional MXC evaluation.

## 3. Non-goals

Do not port the entire IDE, force WSL or Docker, replace `ManagedProcessService` / `JobSubmissionService`, ship a new daemon, change TUI settings semantics, promise network isolation from Windows token restrictions alone, modify global ACLs as a default permanent side effect, or accept unqualified MXC as security authority. Windows low-integrity-only, Job-only or command-filter-only execution is **not** sufficient to mark constrained filesystem sandbox available.

## 4. Current state (repo evidence)

- `src/security/sandbox/backend.rs`: registry exposes Landlock/Seatbelt only and `apply(spec)` assumes mutate-helper + `exec`.
- `src/bin/codegg-sandbox-helper.rs`: `cfg(not(unix))` exits unavailable; Unix private FD status.
- `src/managed_process.rs`: constrained launch is rejected on Windows; unconstrained launch uses suspended child + Job Object ownership, with hosted process-tree tests.
- `src/tool/bash/process.rs`: raw Windows shell argv selects PowerShell by default and supports explicit cmd/Git Bash/WSL; `src/security/sandbox/policy.rs` now supplies Windows runtime/cache/credential inputs and rejects common unsafe path forms before canonicalization.
- These policy checks are preparation only. There is still no Windows filesystem enforcement backend or native Windows security matrix; `.github/workflows/ci.yml` uses Ubuntu, with process and policy-input coverage in the focused Windows workflow.
- `crates/codegg-client/src/windows_process.rs` has a narrow existing liveness probe; Windows named-pipe ACL implementation is client IPC, not sandbox authority.

## 5. Target architecture

`ExecutionPolicySnapshot / SandboxProfile` → `SandboxConfig`/typed capability requirements → `ManagedProcessService::run` → `PlatformSandboxLauncher` → { Unix trusted helper + apply + exec | Windows suspended restricted child + Job Object + status channel } → observed enforcement/audit. No privilege-changing preparatory side effects occur inside the agent's unrestricted command path.

Windows file policy reconciles workspace and runtime read roots, exact write roots, explicitly denied sensitive data, NTFS DACLs and any restricted security principal. Canonicalize/normalize all Windows path forms, then validate *handle-based* paths and reparse points for the operations that require such validation. Per-run isolation configuration must be immutable and independently attributable to job/attempt.

## 6. Dependency graph

`M001` launcher contract [hard] → `M002` supervised Win32 runner [hard] → `M003` native command & policy compatibility [hard] → `M004` filesystem enforcement [hard] → `M006` qualification/release [hard].

`M004` → `M005` separate-user offline/network isolation [hard for offline guarantee; *not* hard for file-only M006]. **ADR-0020 exception for experimentation:** C021 may test a dedicated-user *filesystem candidate* before M004 closure on a disposable VM; if later selected, split identity foundation as an M004 input while retaining M005's firewall/offline verification afterward. This does not make M005 ready. M006 qualifies offline behavior only when M005 is closed.

`M001` → `M007` MXC evidence-only feasibility [interface; optional, no dependency of M002–M006].

Existing ADR-0004, canonical managed-process ownership, and durable policy snapshot are stable interfaces. Runtime-safety's historical Linux evidence condition is *not* a hard gate for adding Windows; do not reopen that workstream's status or fabricate Linux closure.

## 7. Milestones

### M001 — Launcher contract, policy capability and failure gating
Class: invariant. Objective: make restricted Windows launch expressible without Unix `exec` assumptions. Deliverable: typed process-launch seam, truthful per-dimension guarantee requirements and consent-aware unavailable behavior. Exit: static/contract tests and unchanged Unix tests. Non-goal: executable Windows containment.

### M002 — Win32 supervised launcher and Job ownership
Class: infrastructure. Objective: process control and trustworthy launch lifecycle, but no filesystem-enforced claim. Deliverable: suspended unconstrained child creation, Jobs, explicit stdio inheritance, cancellation/reap and output limits; constrained Windows requests remain unavailable before spawn. Restricted-token launch and a protected enforcement-status channel are owned by M004, when a consumer and truthful status exist. Exit: Windows live process-tree, timeout and fail-closed request tests. Non-goal: mark SandboxProfile enforced.

### M003 — Native shell dispatch and platform policy inputs
Class: infrastructure. Objective: reliable Windows-native command construction and runtime/credential path classifications. Deliverable: direct argv, cmd/PowerShell opt-in shell selection, path, env and allowances tests. Exit: Windows command/reparse-path fixtures; clear FullHost vs constrained behavior. Non-goal: filesystem enforcement itself.

### M004 — Restricted-token + filesystem enforcement
Class: capability. Objective: observed `ReadOnly`/`WorkspaceWrite` restricted access and credential-read denial. Deliverable: security token + ACL-scoped root strategy or rigorously justified alternate native enforcement if required. Exit: observed read/write/deny, descendant and cleanup tests; **stop** if mechanism cannot satisfy read restrictions. Non-goal: network isolation.

### M005 — Optional dedicated identity + firewall-enforced offline execution
Class: capability. Objective: separately approved provisioned identity and actual offline network restriction. Deliverable: reversible provisioning, identity-scoped firewall, explicit online/offline capability reporting. Exit: live socket/DNS/loopback and rollback evidence. Non-goal: requiring administration for ordinary file-only runs.

### M006 — Native Windows security CI, operator integration and release gate
Class: capability. Objective: end-to-end qualified native Windows sandbox with a documented support matrix, runnable fixtures, packaging and auditable operator reports. Depends hard on M004; M005 only for advertising offline support. Exit: negative security tests on genuine Windows runners, Linux/macOS regressions, release gating, closure record. Non-goal: claim unsupported network isolation.

### M007 — MXC implementation feasibility (optional)
Class: infrastructure. Objective: non-production comparison of Microsoft MXC Rust SDK and Windows ProcessContainer against Codegg's policy contract. Exit: reproducible compatibility/security gap matrix and explicit accept/defer/reject disposition; no production auto-selection. Non-goal: production security promise.

## M004 corrective execution sequence

M004 is **blocked**, with its historical failure recorded immutably at `plans/closure/windows-native-sandbox/004-status.md`. Rather than changing or pretending to close that record, four bounded corrective plans are now registered:

- **C008 (closed):** `plans/implementation/windows-native-sandbox/008-appcontainer-ffi-attribute-lifetime-corrective.md` — owned stable `SECURITY_CAPABILITIES` and profile SID lifetime; native Windows Server 2025 token/Job launch and suspended-failure tests passed in `plans/closure/windows-native-sandbox/008-status.md` (workflow `38178437505`).
- **C009 (closed):** `plans/implementation/windows-native-sandbox/009-restricted-launch-managed-process-integration.md` — test-only restricted child/stdio/status/cancellation integrated through `ManagedProcessService`; hosted workflow `38196751443` and Unix quick verification passed. This is *not* file containment activation; see `plans/closure/windows-native-sandbox/009-status.md`.
- **C010 (closed; test-only infrastructure):** `plans/implementation/windows-native-sandbox/010-windows-acl-leases-and-recovery.md` — owner-bound ACL leases and recovery passed same-revision Windows Server 2025 workflow `38215171043` on `83c29a12`. Production admission remains unavailable.
- **C011 (blocked corrective qualification):** `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md` — hosted runs `38217933472` and `38220136385` read a denied credential directly, through a junction, and through a hardlink. C012's bounded test-only rejection passed, but C011 still requires race-safe path handling and the privileged IPC, descendant, restart, and realistic workspace matrix. The high-severity finding keeps M004 blocked.
- **C012 (conditionally closed; test-only fail-closed preflight):** `plans/implementation/windows-native-sandbox/012-existing-tree-deny-lease-correction.md` — nonempty deny roots now fail before child creation; separate empty-root positive behavior passed exact-SHA Windows run `38223080719`. The preflight is path-based and does not qualify replacement races or enable production backend admission; see `plans/closure/windows-native-sandbox/012-status.md`.

- **C013 (closed negative; successor deferred):** `plans/implementation/windows-native-sandbox/013-c011-existing-object-access-and-alias-containment.md` and `plans/closure/windows-native-sandbox/013-status.md` — exact-SHA workflows `38296752862` and `38299806236` found the root lease mutates a shared hardlink-backed object and leaves descendant DACL control-bit drift after restoration; grant-only forensic access did not preserve the fixture's allowed read. Populated roots continue to fail closed before child creation in test support. Proposed ADR-0019 scopes a future staged projection or isolated identity review; no mechanism is accepted. C011/M004 and downstream work remain blocked.

**Dependency class:** C008 → C009 → C010 → C011 is **hard**; C012 is a corrective response to C011's hosted leak and is conditionally closed only for test-only fail-closed preflight. C013 is closed negative; ADR-0019 accepted only the bounded AppContainer feasibility experiment. C014 then closed negative because minimum Git initialization could not run within the tested grants; accepted ADR-0020 authorizes only test-only C019–C022 comparative feasibility; existing M004/C011 gates remain blocked. C011 remains blocked on independent privilege, IPC, descendant, restart, realistic-workspace qualification and review. A qualifying elevated disposable Windows runner satisfies synthetic-fixture diagnosis, not the isolated-VM requirement for strict M004 requalification. ADR-0016 and M001–M003 are accepted/stable **interface** dependencies. Optional M007 MXC remains separate and non-blocking. Do not turn historical `004-status.md` from blocked into closed; write a new additive requalification record only after C011's remaining evidence is accepted.

**Root-cause hypothesis:** `src/managed_process/windows.rs` `WindowsProcessAttributes::set_appcontainer` passes an address of a function-local `SECURITY_CAPABILITIES` into `UpdateProcThreadAttribute` and returns before `CreateProcessW`. The documented Win32 contract requires `lpValue` to survive until `DeleteProcThreadAttributeList`; the observed error 87 may also have additional causes. Repair and test ownership before modifying more command-line flags or current-directory settings.

**Availability and acceptance:** Until C011 establishes requested read/write guarantees and approved backend registration, Windows `SandboxRequest::Required` stays unavailable/fail-closed. Successful C008/C009/C010 infrastructure tests never mean `filesystem=Enforced`. Keep `network=Unrestricted` absent separately qualified M005. Maintain same-revision Windows Server 2025 native testing and Linux/macOS regression guards; no VM results may be inferred from hosted-runner tests. At branch merge time, reconcile independently advancing `main` without folding unrelated human-shell runtime work into a fictitious Windows security closure.

## ADR-0019 projection successor milestones

ADR-0019 is **accepted only as a bounded staged-projection/AppContainer research direction**. This does *not* establish a production filesystem sandbox. The known C011 populated-tree credential-read failure and C013 hardlink/control-bit findings remain open and historically immutable. Do not modify existing user workspace ACLs. Keep Windows production `SandboxRequest::Required` unavailable and obtained filesystem enforcement unclaimed.

**Dependency chain (hard):** C014 feasibility → C015 bounded independent-file snapshot → C016 AppContainer/Job staged execution → C017 authorized output promotion → C018 native adversarial qualification and C011 re-entry. All work is on the original `work/windows-native-sandbox-foundation` branch; review of additional unrelated diff is explicitly outside this workstream.

- **C014 (closed negative; ADR review proposed):** `plans/implementation/windows-native-sandbox/014-projection-appcontainer-compatibility-and-deny-boundary.md` and `plans/closure/windows-native-sandbox/014-status.md` — exact-SHA Windows Server 2025 run `38316065754` confirmed PowerShell could read/write a disposable projection, but Git initialization failed because AppContainer access to `/dev/null` was denied. Git is the minimum required tool, so the selected tool subset is not viable under the tested grants. Host-alias, IPC and inherited-handle probes did not run. C015–C018 remain blocked; accepted ADR-0020 authorizes a finite MXC v1 versus dedicated-principal feasibility comparison, not a production backend. No production sandbox is enabled.
- **C015 (blocked on strict C014):** `plans/implementation/windows-native-sandbox/015-bounded-snapshot-materialization-and-input-manifest.md` — independent new-file copies only, reject hardlink/reparse/ADS and sensitive/unsupported content, handle-verified source identity, bounded private manifest and cleanup; no `CopyFileExW` metadata propagation.
- **C016 (blocked on strict C015):** `plans/implementation/windows-native-sandbox/016-appcontainer-projection-launch-and-toolchain-qualification.md` — invoke validated tools through the existing `ManagedProcessService` AppContainer/Job boundary with projection cwd, no unqualified original-root grants and actual denied child+descendant probes.
- **C017 (blocked on strict C016):** `plans/implementation/windows-native-sandbox/017-authorized-projection-output-reconciliation.md` — stage output inventory and separate, authorized, freshness-checked promotion to the actual worktree, with preview, dirty-file conflicts, link/race protection and rollback.
- **C018 (blocked on strict C017 + independent VM):** `plans/implementation/windows-native-sandbox/018-projection-security-qualification-and-c011-reentry.md` — hosted and privileged isolated-VM security matrix, C011/M004 re-entry decision, clear status limits; no production backend without separate strict acceptance.

**Status truth:** passing a copy test, running tool, or applying authorized output is not proof of OS filesystem isolation. Only real running-child denial + explicitly authorized output promotion + C011 independent security review could justify requalification. M004/M005/M006 remain blocked; M007 remains a historically deferred pinned-preview experiment; separate C020 requalifies MXC v1 without changing M007 closure. Experimental test-only code and metadata are not publicly usable constrained execution. Network guarantees remain separately unclaimed.

**Research:** `research/windows-native-sandbox/adr-0019-successor-assessment.md`. Microsoft AppContainer access checks (including certain default OS access), NTFS hardlink shared descriptors, and `CopyFileExW` metadata/ADS copying are concrete input- and runtime-policy hazards. OpenAI's Windows Codex report found AppContainer troublesome for general development workflows, so compatibility is a **first** gate, not an assumed eventual success. Dedicated sandbox-user provisioning is a deferred alternative requiring separate administrator consent/ADR, not an implicit fallback.

## ADR-0020 independent successor feasibility (2026-10-12)

**Accepted decision: comparison only.** After C013's populated-tree ACL mutation and C014's negative Git/projection AppContainer experiment, preserve both historical closure records. Production Windows `SandboxRequest::Required` stays unavailable; filesystem/network `Enforced` are not advertised. Historical ADR-0019 C015–C018 stays dependency-blocked and is **not** the default successor path. Reference `research/windows-native-sandbox/adr-0020-harness-comparison.md`.

**Dependency graph:** C019 common running-child oracle and C014 device diagnosis [ready, hard] → {C020 MXC SDK v1 native PSEC candidate [hard C019, operational serviced Windows 11], C021 dedicated low-privilege local principal [hard C019, operational admin-consented disposable Windows VM]} [independent parallel closures] → C022 evidence comparison/next-admission decision [hard C020/C021 written dispositions]. Missing host = blocked; actual unsafe access/incompatibility = closed negative. Neither is a production admission gate.

- **C019 (ready):** `plans/implementation/windows-native-sandbox/019-shared-native-security-probe-contract.md`. Separate compatibility and adversarial probes so Git failure does not suppress host/alias/IPC tests; baseline C011/C013/C014, verify `\Device\Null` root-cause hypothesis on disposable host, test-only report contract.
- **C020 (blocked):** `plans/implementation/windows-native-sandbox/020-mxc-v1-native-process-isolation-feasibility.md`. Independently pin upstream MXC v1.0.0/SDK API, selected *native tier* and exact Windows build servicing; reject AppContainer+DACL fallback requiring original host mutation, do not raise Codegg MSRV, test running child against C019 matrix. Historical M007 remains deferred.
- **C021 (blocked):** `plans/implementation/windows-native-sandbox/021-dedicated-user-native-feasibility.md`. Disposable provisioned local user, restricted token/Job, separate setup-vs-agent authority and owned new-file stage; test *actual read denial*, not only `WRITE_RESTRICTED` write semantics. No production account creation, host ACL lease, or firewall claim.
- **C022 (blocked):** `plans/implementation/windows-native-sandbox/022-feasibility-comparison-and-successor-admission-decision.md`. Record chosen feasible architecture / neither / evidence-blocked. Any future production integration, M004/C011 re-entry, source projection/promotion reuse or M005 dependency split needs a separate maintainer-approved decision.

Microsoft MXC SDK `v1.0.0` published 2026-10-07 after old M007 preview pin; its Rust build toolchain is 1.93 versus Codegg MSRV 1.89. Microsoft now documents serviced Windows 11 Process Isolation (as of research: 24H2 26100.9278, 25H2 26200.9278), but host *edition/build/KB*, native tier, policy honor and sandbox child behavior remain decisive. VS Code's Windows Agent Host sandbox is experimental and OS-update-dependent. MXC's SDK ABI stability is not security validation. Reference https://github.com/microsoft/mxc/releases/tag/v1.0.0 and https://code.visualstudio.com/docs/agents/run/agent-sandboxing .

**No historical gate is weakened:** C011/M004–M006 blocked, C015–C018 blocked, source ACL/DACL/SACL/MIC unchanged, no unverified offline/network guarantee, no workaround granting credential-bearing user caches or changing global device security descriptors. Independent VM security review is still required before *any* future production admission.

## 8. Cross-cutting requirements

### Storage and migration
Prefer ephemeral per-run identities and directories; durable installed account/ACL/firewall provisioning requires stable owner/version, explicit consent and idempotent recover/repair/remove. Do not persist token handles. Use current audit fields; schema migration only if typed obtained guarantees cannot be represented.

### Protocol and compatibility
Honor `SandboxProfile`, `ApprovalMode`, permission receipts and immutable execution-policy snapshots. Preserve existing Unix helper status decoding or version a Windows-specific private status channel deliberately. No new public agent protocol without evidence.

### Security and authorization
Restricted tokens alone do not imply read isolation; `WRITE_RESTRICTED` permits reads subject to ordinary ACLs. NTFS reparse points, inherited ACLs, symlinks, junctions, alternate data streams, UNC/device namespace, executable/DLL lookup, inherited handles and daemon named pipes need negative probes. Every missing mechanism produces a typed unavailable/denied result, not a misleading `Enforced`.

### Concurrency, cancellation and recovery
Job handles own descendants; no `CREATE_BREAKAWAY_FROM_JOB` leakage, suspend-before-restrict, lockable per-run ACL leases, cleanup after timeout/crash/restart, and bounded helper/stdout/status pipes. Cross-worktree roots must not be combined.

### Observability and audit
Distinguish requested profile, backend version, actual token identity, ACL scope, filesystem reads/writes, network state, Job existence, and unavailable reason; redact SIDs/paths where security sensitive. UI descriptions are not proof.

### Performance and resource use
Limit process count/memory/job lifetime; record startup latency relative to Linux/macOS and cost of per-invocation ACL changes. No heavy background bootstrap per command.

### Documentation and operations
Update `architecture/security.md`, `architecture/process-tool-execution-ownership.md`, `architecture/tool.md`, `architecture/testing.md` and installation docs only when functionality lands.

## 9. Verification strategy

Test policy/state machine on all hosts; Windows native integration on GitHub-hosted Windows and a separately provisioned Windows test fixture for ACL and firewall tests. Execute harmless native commands through production path and attempt prohibited effects from an **untrusted child**, not only validate generated JSON. Compare effective token, Job membership, inherited descendant restrictions, ACL after recovery, denied reads, directory reparse escape, network when enabled, daemon named pipe and terminated descendants. Native test absence is a named blocker, never a pass. Linux Landlock/macOS Seatbelt tests stay in normal guards.

Reference APIs: Microsoft `CreateRestrictedToken`, `CreateProcessAsUserW`, `AssignProcessToJobObject` and `CreateAppContainerProfile` documentation; current preview `https://github.com/microsoft/mxc` (explicitly unqualified). See ADR-0016.

## 10. Risks and decision points

- Strong Windows arbitrary read denial may require a different token/identity/AppContainer arrangement than writable-roots isolation; M004 must stop and record a corrective design decision rather than mislabel partial isolation.
- Shell wrappers, signed/system toolchains, `%TEMP%`, package caches, MSVC/Rust/Python paths and DLL loads can pressure allowlists.
- Privileged provisioning and firewall rules create durable host state; require independent admin-facing consent and rollback.
- Running Windows tests in CI may not allow provisioning; use a separate opt-in VM fixture and keep standard runner tests non-administrative.
- MXC SDK v1.0.0 (2026-10-07) and VS Code's experimental native Windows integration warrant C020 renewed investigation on *serviced Windows 11*, not automatic approval; runtime tier and actual denied-path guarantees must be proven, and upstream still cautions generated profiles may be overly permissive. Do not infer Windows 11 Process Isolation from Windows Server 2025's base build.

## 11. Completion definition

Codegg on a documented Windows version supports constrained local tool execution through the authoritative process service with verified filesystem read/write restrictions, reliable descendant cleanup, correct approval/audit semantics, no silent fallback, and native Windows security evidence. Network containment is claimed only after M005 proves it. Unimplemented Windows versions show a precise capability/unavailability reason.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/windows-native-sandbox/001-launch-contract-and-fail-closed-authority.md` | `plans/closure/windows-native-sandbox/001-status.md` | none |
| M002 | closed | `plans/implementation/windows-native-sandbox/002-win32-launch-and-job-ownership.md` | `plans/closure/windows-native-sandbox/002-status.md` | none |
| M003 | closed | `plans/implementation/windows-native-sandbox/003-native-shell-and-windows-policy-inputs.md` | `plans/closure/windows-native-sandbox/003-status.md` | M002 strict closure satisfied; native Windows workflow `38118934632` and Unix quick verification passed |
| M004 | blocked | `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md` | `plans/closure/windows-native-sandbox/004-status.md` | Historical error-87 evidence is retained; C008 repairs token/Job launch and C010 supplies test-only ACL lease/recovery infrastructure, but no production backend caller is approved and C011 privileged adversarial evidence is outstanding |
| M005 | blocked | `plans/implementation/windows-native-sandbox/005-dedicated-identity-and-offline-network.md` | — | M004 |
| M006 | blocked | `plans/implementation/windows-native-sandbox/006-windows-security-qualification-and-release.md` | — | M004; offline scope additionally M005 |
| M007 | deferred historical preview | `plans/implementation/windows-native-sandbox/007-mxc-feasibility-spike.md` | `plans/closure/windows-native-sandbox/007-status.md` | No qualifying Windows 11 SDK build/runtime matrix; defer with no security conclusion |
| C008 | closed | `plans/implementation/windows-native-sandbox/008-appcontainer-ffi-attribute-lifetime-corrective.md` | `plans/closure/windows-native-sandbox/008-status.md` | Workflow `38178437505`; Windows Server 2025 Rust ABI/token/Job proof and Unix quick verification passed; C009 unblocked |
| C009 | closed | `plans/implementation/windows-native-sandbox/009-restricted-launch-managed-process-integration.md` | `plans/closure/windows-native-sandbox/009-status.md` | Workflow `38196751443` passed on Windows Server 2025; restricted `ManagedProcessService` integration and Unix quick verification passed. Production constrained requests remain unavailable. |
| C010 | closed (test-only infrastructure) | `plans/implementation/windows-native-sandbox/010-windows-acl-leases-and-recovery.md` | `plans/closure/windows-native-sandbox/010-status.md` | Windows Server 2025 run `38215171043` passed owner-bound ACL lease and recovery tests; production backend remains unavailable. |
| C011 | blocked | `plans/implementation/windows-native-sandbox/011-m004-native-adversarial-qualification.md` | `plans/closure/windows-native-sandbox/011-status.md` | Hosted runs `38217933472` and `38220136385` demonstrated direct, junction, and hardlink credential reads through a denied path; C012 closes only the bounded pre-spawn rejection slice; C013 and C014 closed negative. C011's remaining security matrix and independent review are still required. Accepted ADR-0020 registers C019 common tests (ready), C020 MXC v1 and C021 dedicated principal (hard C019 and operational VM prerequisites), then C022 comparison; C011 remains blocked. |
| C012 | conditionally closed | `plans/implementation/windows-native-sandbox/012-existing-tree-deny-lease-correction.md` | `plans/closure/windows-native-sandbox/012-status.md` | Exact-SHA Windows run `38223080719` passed nonempty-deny-root preflight rejection, DACL preservation, separate empty-root positive tests, and C010 lease tests. Concurrent path replacement and the full C011/M004 matrix remain unqualified. |
| C013 | closed (negative feasibility; successor deferred) | `plans/implementation/windows-native-sandbox/013-c011-existing-object-access-and-alias-containment.md` | `plans/closure/windows-native-sandbox/013-status.md` | Windows Server 2025 workflows `38296752862` and `38299806236`: populated root lease changes a shared hardlink object and leaves descendant control-bit drift; grant-only allowed read did not pass. ADR-0019 later authorized bounded C014 feasibility, which also closed negative. Populated roots fail closed in test support; C011/M004 remain blocked. |
| C014 | closed (negative feasibility; ADR-0020 accepted experiment comparison) | `plans/implementation/windows-native-sandbox/014-projection-appcontainer-compatibility-and-deny-boundary.md` | `plans/closure/windows-native-sandbox/014-status.md` | Windows Server 2025 run `38316065754` proved disposable projection read/write, but minimum Git smoke failed opening `/dev/null` under AppContainer; host-denial probes were not reached. Accepted ADR-0020 (feasibility only) compares dedicated identity with retaining fail-closed unavailability. C015–C018 remain blocked. |
| C019 | active | `plans/implementation/windows-native-sandbox/019-shared-native-security-probe-contract.md` | — | Common actual-child oracle, old bypass baselines and Git/null-device diagnosis; no production path |
| C020 | blocked (hard + operational) | `plans/implementation/windows-native-sandbox/020-mxc-v1-native-process-isolation-feasibility.md` | — | C019 strict closure and eligible serviced Windows 11 test host; historical M007 deferred |
| C021 | blocked (hard + operational) | `plans/implementation/windows-native-sandbox/021-dedicated-user-native-feasibility.md` | — | C019 strict closure and operator-consented disposable admin VM |
| C022 | blocked | `plans/implementation/windows-native-sandbox/022-feasibility-comparison-and-successor-admission-decision.md` | — | Independent written C020/C021 dispositions and C019 evidence; later production ADR required |
| C015 | blocked | `plans/implementation/windows-native-sandbox/015-bounded-snapshot-materialization-and-input-manifest.md` | — | C014 closed negative; requires an accepted successor architecture decision and a newly qualified strict-positive feasibility gate |
| C016 | blocked | `plans/implementation/windows-native-sandbox/016-appcontainer-projection-launch-and-toolchain-qualification.md` | — | C015 strict positive bounded snapshot closure |
| C017 | blocked | `plans/implementation/windows-native-sandbox/017-authorized-projection-output-reconciliation.md` | — | C016 strict positive restricted projection execution |
| C018 | blocked | `plans/implementation/windows-native-sandbox/018-projection-security-qualification-and-c011-reentry.md` | — | C017 strict positive approval/promotion + independent Windows privileged VM |
