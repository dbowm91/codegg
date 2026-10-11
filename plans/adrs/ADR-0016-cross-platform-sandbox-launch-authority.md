# ADR-0016: Platform-Owned Sandbox Launch and Obtained-Enforcement Authority

Status: accepted

Date: 2026-10-10

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Affected subsystem roadmaps:

- `plans/subsystems/windows-native-sandbox-roadmap.md`
- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` (cross-reference only)

Related ADRs: `ADR-0001`, `ADR-0002`, `ADR-0004`. This decision **complements**, does not supersede, ADR-0004's approval/profile separation or its explicit escalation rule.

## Context

At baseline `8e9d8b01`, `src/security/sandbox/backend.rs` assumes `apply(spec)` changes the current helper process followed by Unix `exec`; `src/bin/codegg-sandbox-helper.rs` and `src/managed_process.rs` reject native Windows `SandboxRequest::Required`. `src/tool/bash/process.rs` assumes `sh -c`. Windows normally launches a separate process with a preconfigured token/security environment. Reusing the Unix apply-and-exec hook as a nominal Windows registry entry would be a false security claim.

`SandboxProfile` names *requested* filesystem authority, distinct from `ApprovalMode`. `SandboxLaunchOutcome::Enforced` and `SandboxEnforcement` must describe *obtained*, mechanism-specific guarantees. Existing absence-of-backend behavior can produce a `DegradedUncontained` command; ADR-0004 already requires that this must never become automatic host execution without explicit operator-approved escalation.

## Decision drivers

- Do not run untrusted target code before required containment is installed and verified.
- Keep one scheduler/`ManagedProcessService` execution owner, even when Windows needs Win32 process handles and Jobs.
- Preserve existing Unix Landlock/Seatbelt code and statuses without requiring their launcher design on Windows.
- Prevent `WRITE_RESTRICTED`-only tokens, Job Objects, or a portable Python policy from being mistaken for full filesystem or network containment.
- Preserve immutable effective principal/workspace/subagent ceilings and auditable, explicit `FullHost` authority.
- No elevation, firewall change, host ACL widening, or local user creation through implicit agent action.

## Considered options

### A. Extend current `apply` callback on Windows

Smallest type change; rejected because creating another Windows process is not an in-place `exec` replacement, and pre-resume supervision/status cannot be modeled honestly by `apply`.

### B. Platform-specific launch adapters behind the canonical managed-process owner (selected)

The sandbox subsystem compiles a backend-neutral *requested* policy; the platform-owned launcher returns typed setup and observed-enforcement evidence. Unix keeps its current one-shot helper/apply/exec path. Windows launches suspended, configures token, Job Object and restricted handles, and resumes only after all required setup succeeds. Existing managed-process lifecycle owns cancellation, output and provenance in both cases.

### C. Invoke WSL/VM for all Windows commands

Strong isolation is possible with additional runtimes, but is not native Windows execution and creates unwarranted install/translation dependencies. Deferred as optional execution targets.

### D. Depend on Microsoft MXC preview immediately

The current Microsoft MXC preview explicitly warns that generated policies can be overly permissive and should not be used as security boundaries; Windows denied-path fidelity is incomplete. Not accepted as an initial security authority; independently evaluated later.

## Decision

1. Define one internal launcher boundary used by `ManagedProcessService`: launch request (executable/argv/cwd/env/stdin/provenance/timeout plus requested sandbox policy) → owned child/job handles, private status channel, and typed obtained enforcement. Backend implementations may apply in place only if platform execution semantics support it; they may create restricted child processes instead.
2. `SandboxProfile::{ReadOnly,WorkspaceWrite,FullHost}` and `ApprovalMode` remain independent, with unchanged daemon/turn/subagent authority. A constrained request with unavailable, partial, or failed enforcement **cannot** execute an uncontained target unless a distinct explicit, authorized escalation selects the corresponding authority under ADR-0004; log-only degradation is insufficient.
3. Filesystem read, filesystem write, network egress, process-tree control and remaining limits are separately reportable obtained facts. Never infer one dimension from another. Only label a requested profile enforced after the backend demonstrates **all** its required capabilities under its specified contract. Any narrower mechanism must be reported as partial/unavailable, or rejected.
4. `ManagedProcessService` remains the single local process owner. Windows child setup must be complete before `ResumeThread`; Job Object assignment, kill-on-close behavior, restricted-token launch, inherited handle restrictions and status handoff are all fail-closed gates.
5. Windows filesystem policy must be evaluated against native NTFS DACLs, token access checks and real filesystem handles, including reparse points; path string normalization is only an input validation layer.
6. Administrative provisioning for dedicated identities/firewall controls is optional, outside model-driven tool execution, reversible, and independently authorized. A native Windows file-only sandbox may advertise `network=Unrestricted`, not a network deny.
7. Unix security contracts, artifact path resolution, trusted sibling helper rules, CLI/TUI/daemon authority, and storage ownership remain unchanged unless a migration is explicitly warranted.
8. Prefer Rust `windows-sys` or equivalently maintained Win32 bindings in narrow `cfg(windows)` modules, with scoped, reviewed unsafe and RAII handle cleanup. No Windows privilege logic in shared protocol or pure domain crates.

## Consequences

### Positive

Native Windows launch can be implemented without weakening Unix containment. Setup-time proofs and runtime supervision share the existing execution boundary; future backends can be compared on obtained guarantees rather than claimed availability.

### Negative

Launch and status interfaces require a deliberate split; Windows may initially have a restricted subset of toolchains. Real Windows test runners are mandatory for security qualification.

### Neutral or deferred

Separate-user offline networking, AppContainer, MXC, WSL2 and VM backends remain optional; no future backend is preselected by this ADR. Do not add a durable public protocol simply to carry private helper configuration.

## Compatibility and migration

Retain serialization and observed behavior of existing Unix `SandboxLaunchSpec` / status frames or version them explicitly if shared wire frames change. No persisted token SIDs, ACLs, or secrets in model transcript. FullHost remains a deliberately selected profile. Existing unsupported-host degraded behavior must be gated by the explicit-escalation contract before any Windows sandbox can be presented as available.

## Security and reliability implications

Use narrowly inherited handles, dedicated protected status channel, no target-controlled setup or helper selection, fail-closed token/job/ACL/identity failures, cleanup after timeout/cancellation/restart, and no durable broad ACL grants after teardown. Jobs limit process scope and resources but do not provide filesystem or network isolation. Security qualification must prove shell/toolchain descendants inherit the intended Windows token and job restrictions.

## Verification

Unit tests for requested-vs-obtained policy, dynamic Windows backend capability, no silent `DegradedUncontained`; native tests for denial, process descendants, cleanup, ACL lifecycle and IPC access; unchanged Linux/macOS regression tests. Every security guarantee must be supported by observed target behavior and evidence in closure records, not solely self-reported helper status.

## Supersession

None. Reassess when a later Windows backend can be independently proven to satisfy stronger contract requirements.
