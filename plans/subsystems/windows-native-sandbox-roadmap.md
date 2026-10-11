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
- `src/managed_process.rs`: required sandbox launch rejected under `cfg(not(unix))`; `terminate_child` on Windows kills the immediate child only; explicit `DegradedUncontained` can run directly.
- `src/tool/bash/process.rs`: `sh -c` and `src/security/sandbox/policy.rs`: Unix allowances/HOME assumptions.
- `tests/sandbox_containment.rs` and `tests/sandbox_landlock.rs`: Unix-only fixture behavior; no native Windows security matrix. `.github/workflows/ci.yml` uses Ubuntu.
- `crates/codegg-client/src/windows_process.rs` has a narrow existing liveness probe; Windows named-pipe ACL implementation is client IPC, not sandbox authority.

## 5. Target architecture

`ExecutionPolicySnapshot / SandboxProfile` → `SandboxConfig`/typed capability requirements → `ManagedProcessService::run` → `PlatformSandboxLauncher` → { Unix trusted helper + apply + exec | Windows suspended restricted child + Job Object + status channel } → observed enforcement/audit. No privilege-changing preparatory side effects occur inside the agent's unrestricted command path.

Windows file policy reconciles workspace and runtime read roots, exact write roots, explicitly denied sensitive data, NTFS DACLs and any restricted security principal. Canonicalize/normalize all Windows path forms, then validate *handle-based* paths and reparse points for the operations that require such validation. Per-run isolation configuration must be immutable and independently attributable to job/attempt.

## 6. Dependency graph

`M001` launcher contract [hard] → `M002` supervised Win32 runner [hard] → `M003` native command & policy compatibility [hard] → `M004` filesystem enforcement [hard] → `M006` qualification/release [hard].

`M004` → `M005` separate-user offline/network isolation [hard for offline guarantee; *not* hard for file-only M006]. M006 qualifies offline behavior only when M005 is closed.

`M001` → `M007` MXC evidence-only feasibility [interface; optional, no dependency of M002–M006].

Existing ADR-0004, canonical managed-process ownership, and durable policy snapshot are stable interfaces. Runtime-safety's historical Linux evidence condition is *not* a hard gate for adding Windows; do not reopen that workstream's status or fabricate Linux closure.

## 7. Milestones

### M001 — Launcher contract, policy capability and failure gating
Class: invariant. Objective: make restricted Windows launch expressible without Unix `exec` assumptions. Deliverable: typed process-launch seam, truthful per-dimension guarantee requirements and consent-aware unavailable behavior. Exit: static/contract tests and unchanged Unix tests. Non-goal: executable Windows containment.

### M002 — Win32 supervised launcher and private status handoff
Class: infrastructure. Objective: process control and trustworthy launch lifecycle, but no filesystem-enforced claim. Deliverable: suspended child creation, Jobs, inherited-handle isolation, secure status, cancellation/reap and output limits. Exit: Windows live process-tree, timeout and status tests. Non-goal: mark SandboxProfile enforced.

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
- MXC currently explicitly warns against trusting preview profiles as security boundaries; Windows denied-path handling is not equivalent to Codegg's `deny_paths`.

## 11. Completion definition

Codegg on a documented Windows version supports constrained local tool execution through the authoritative process service with verified filesystem read/write restrictions, reliable descendant cleanup, correct approval/audit semantics, no silent fallback, and native Windows security evidence. Network containment is claimed only after M005 proves it. Unimplemented Windows versions show a precise capability/unavailability reason.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | active | `plans/implementation/windows-native-sandbox/001-launch-contract-and-fail-closed-authority.md` | — | none |
| M002 | blocked | `plans/implementation/windows-native-sandbox/002-win32-launch-and-job-ownership.md` | — | M001 |
| M003 | blocked | `plans/implementation/windows-native-sandbox/003-native-shell-and-windows-policy-inputs.md` | — | M002 |
| M004 | blocked | `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md` | — | M003 |
| M005 | blocked | `plans/implementation/windows-native-sandbox/005-dedicated-identity-and-offline-network.md` | — | M004 |
| M006 | blocked | `plans/implementation/windows-native-sandbox/006-windows-security-qualification-and-release.md` | — | M004; offline scope additionally M005 |
| M007 | blocked/optional | `plans/implementation/windows-native-sandbox/007-mxc-feasibility-spike.md` | — | M001 contract; non-release-critical |
