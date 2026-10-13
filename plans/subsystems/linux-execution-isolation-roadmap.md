# Linux Execution Isolation Roadmap

Status: active (planning; ADR-0022 proposed and downstream implementation gated)

Long-term references:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md`

Related ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016, proposed ADR-0022. Related but independent roadmap: `sandbox-containment-post-closure-corrective-roadmap.md` (C001–C003 closed; C004 conditionally closed); Windows sandbox stays separate.

## 1. Purpose and ownership boundary

Establish verifiable Linux untrusted-child execution containment in the Codegg daemon-owned local execution path. Own Linux sandbox policy compilation, namespace/mount/network/IPC enforcement, trusted Git execution handoff safety, optional resource ceilings and obtained-capability reporting. Consume, but never replace, scheduler/ManagedProcessService process ownership, existing approval/lease authority, the Landlock ABI>=3 child helper, daemon-run Git services, and canonical audit. No agent-selected host/privilege policy.

## 2. Work classification

Invariants: no target executes before *all required* confinement is established; never misreport requested vs obtained filesystem/network/process/resource authority; no shell/agent elevation via Git metadata or executable hooks; child worktree cannot outgrow parent lease; shared daemon/other sessions and credential material stay outside explicit grants; clean cancel/fail/restart accounting.

Capabilities: opt-in protected writable-workspace view; opt-in namespace process isolation; opt-in offline child network, eventually policy-mediated egress; operator-visible reasons for refusal and scoped remedy.

Infrastructure: trusted Linux launch adapter/feature probe, namespace/mount policy compiler, baseline seccomp, optional proxy and cgroup-v2 delegation, native adversarial harness. Polish: concise platform-status diagnostics, mechanism-specific coverage and compatibility measurements.

## 3. Non-goals

No rewrite of the existing working Landlock backend; no change to macOS/Windows defaults, scheduler semantics, TUI/ACP protocol unless a separately versioned DTO is essential, human interactive shell ownership, remote execution or VM construction. No blanket runtime `--ro-bind / /` mistaken for read confidentiality; no setuid helper or root privileges, global firewall modification, arbitrary localhost proxy trust, full shell static parsing or ability to guarantee safety of intentionally user-executed malicious code.

## 4. Current state (main 8535f6d)

- `src/security/sandbox/landlock.rs` requires ABI 3+, `no_new_privs` and reports effective ABI. Native Linux C002/C003 closure records cover read/write/truncate, cross-boundary link, nested credential, private scratch and failure behavior. `SandboxConfig::launch_spec` compiles allowed/denied roots; `validate_landlock_write_exclusions` correctly rejects denied descendants of writable roots.
- `src/managed_process.rs` and `codegg-sandbox-helper` own a private setup channel, constrained per-launch scratch, bounded output and cleanup, but no Linux mount/PID/IPC/network namespaces, seccomp or cgroup owner. `SandboxEnforcement` reports unrestricted network, not a fictional network sandbox.
- `src/security/sandbox/policy.rs` grants curated Linux runtime reads including `/proc`, `/sys`, `/var/run`; these are not equivalent to a private process/IPC view.
- `crates/egggit/src/process.rs` and Git service paths execute privileged-to-the-user Git commands outside the ordinary Bash Landlock boundary. Environment sanitation is useful, but repository-controlled Git hooks, filters and fsmonitor can be executable authority. Agent-written `.git` metadata remains writable under default workspace roots.
- Python Analyze/Verify/Transform policy is separately risk-classified; static network detection is not syscall-enforced isolation. Multi-session daemon/worktree architecture magnifies same-UID IPC and host-process exposure.
- Existing runtime safety roadmap deliberately scoped out namespaces/containers/seccomp in its historical closure; this successor does not reopen it.

## 5. Target architecture

Execution policy snapshot from authenticated daemon authorization -> `ManagedProcessService` canonical launch and lease -> Linux adapter compiles filesystem mount view (read allowlist; writable workspace/scratch; protected Git/secret descendants), PID/IPC/`/proc` scope, optional network namespace/proxy, seccomp and optional cgroups -> helper proves restrictions before target runs -> typed **obtained** dimensions (filesystem read/write; network; process/IPC; resources) + limitations and cleanup state. Existing Landlock-only launch remains truthful and supported; no implicit fallback for a required new tier. Authenticated network policy is an independent dimension, not an alias for ReadOnly/WorkspaceWrite/FullHost. The new backend must not accept workspace/PATH-selected helpers, user-controlled socket/descriptor inheritance, or unsafe mount aliasing.

Trusted Git commands require a distinct reviewed execution route. Ordinary Bash cannot edit executable Git metadata while claiming the new protected-metadata guarantee; changes to index/refs/config that require Git cooperation are admitted through Codegg's owned typed Git service. Resolving worktrees must follow their real gitdir/common-dir, including out-of-workspace paths.

## 6. Dependency graph

`M001 hostile-Git execution boundary` is **ready**, independent of ADR-0022, and produces the privileged-Git threat matrix. `M002 composable launch contract` is **blocked hard** on accepted ADR-0022 (read/review/probes may proceed against proposed interfaces); M001 supplies an **interface** dependency. `M003 protected mount view` hard-depends on M002; M001 closes before its production Git path integration. `M004 process/IPC + seccomp` hard-depends on M002 and has M003 as **interface** dependency. `M005 offline network` hard-depends on M002/M004 process/socket design. `M006 proxy-mediated egress` hard-depends on M005. `M007 cgroup/resource budgets` hard-depends on M002 and can proceed in parallel with M003–M006. `M008 native qualification/default migration` hard-depends on M001–M007 for full qualification; a no-proxy opt-in staged release may close an independently defined subset but cannot claim the full roadmap closed. Host feature availability and exact-revision Linux test runs are **operational** closure gates, not implicit security downgrade permission.

## 7. Milestones

| Milestone | Class | Implementation | Status | User/operator exit |
|---|---|---|---|---|
| M001 Trusted Git execution boundary and hostile-metadata probes | invariant | `implementation/linux-execution-isolation/001-trusted-git-execution-boundary.md` | blocked: authorized contained route needed | Safe policy blocks repository-controlled executable settings, but also refuses ordinary typed filter/merge/network operations; maintainer must select a contained exception route or accept the compatibility loss before closure |
| M002 Composable Linux launch and obtained-capability contract | infrastructure | `implementation/linux-execution-isolation/002-composable-linux-launch-contract.md` | blocked: ADR-0022 acceptance | One canonical launcher and truthful dimensioned status, verified fail-closed probes with unchanged default behavior |
| M003 Protected workspace mount view | capability | `implementation/linux-execution-isolation/003-protected-workspace-mounts.md` | blocked: M002/M001 | Workspace writes work, but configured Git/agent credential descendants cannot be rewritten; overlays and real gitdirs protected |
| M004 Process, IPC and seccomp containment | capability | `implementation/linux-execution-isolation/004-process-ipc-seccomp.md` | blocked: M002; M003 interface | Host processes, daemon sockets and host proc are inaccessible under an opted-in isolated tier |
| M005 Offline network isolation | capability | `implementation/linux-execution-isolation/005-offline-network-isolation.md` | blocked: M002/M004 | Opt-in offline-required denies TCP/UDP and named network escape channels without silent host fallback |
| M006 Mediated allowlisted egress | capability | `implementation/linux-execution-isolation/006-mediated-egress.md` | blocked: M005 | Approved targets only through trusted proxy/DNS routing; no direct socket, local service or UDS bypass |
| M007 Bounded Linux resource envelopes | capability | `implementation/linux-execution-isolation/007-resource-envelopes.md` | blocked: M002 | CPU/memory/pids limits, cleanup and truthful unavailable/degraded status where delegation is absent |
| M008 Qualification, compatibility and rollout gate | invariant | `implementation/linux-execution-isolation/008-qualification-and-rollout.md` | blocked: M001–M007 | Exact-revision native security evidence, documented tool compatibility, explicit adoption/rollback gate |

## 8. Cross-cutting requirements

Storage/migration: prefer internal additive launch configuration initially; do not change persistent profiles or defaults without explicit migration plan. Public protocol changes must be versioned and independently reviewed. Keep operator authority and immutable parent/child ceilings.

Security: no network/IPC capability claim from Landlock filesystem success, raw `/proc` path allowance, static Python analysis or host firewall assumptions. Landlock 4 TCP port control and ABI 6 signal/abstract UDS scopes are useful defense-in-depth only where supported; never a substitute for UDP isolation, unix pathname socket restrictions or hostname allowlisting. Harden mount and proc views with real inode and namespace checks; do not bind host daemon and agent sockets.

Concurrency/recovery: one owned process tree per attempt, scratch & namespace lifetime bounded to it, cgroup teardown and proxy lifecycle scoped to job/session leases. Kill descendants and retain failure evidence before cleanup; no cross-session scratch/FD sharing. Restart must reconcile only owned stale resources and avoid deleting another live attempt.

Observability: requested policy, obtained filesystem/network/process/resource dimensions, backend/tool version, Landlock ABI, restricted scope, unsupported kernel feature and refusal phase; no raw credentials, private proxy credentials, absolute secret paths or unredacted command payloads in ordinary logs.

Compatibility: run real `git`, `cargo`, `rustc`, Python, shell, tests, common LSP/test paths on supported Linux distributions. Address bubblewrap/userns AppArmor/SELinux restrictions and no-userns hosts with actionable refusal; ordinary Landlock-only remains supported but never mislabeled as enhanced. No unconditional full-ci-per-platform matrix.

## 9. Verification strategy

Use focused kernel-faithful child probes in `tests/sandbox_containment.rs` / `tests/sandbox_landlock.rs` plus new Linux-only adversarial targets. Distinguish static plan/compilation tests from native execution success; all claims require actual hostile-operation failure and permitted operations success. Add no-userns negative fixture, hardlink/symlink/path-replacement, real gitdir, inherited FD, host `/proc`/socket, loopback and remote TCP/UDP, proxy DNS rebinding, cancellation/restart, worker/sibling contention, memory/pids quota probes. A supported Linux host must report kernel/version, effective features, binary identity and actual obtained status.

## 10. Risks and decisions

- ADR-0022 **proposed**; operator must accept the composed Linux backend/network intent contract before M002 production promotion. Do not self-accept during code implementation.
- Bubblewrap relies on unprivileged user namespaces; locked-down hosts and WSL1 may reject it. Do not invent a bypass or use setuid wrapper; retain truthful legacy tier.
- Mount ordering/alias/overlays may conceal gitdir or allow indirect writes; require real security fixtures and refusal on ambiguity.
- Rust/Cargo builds, Git worktrees, LSP/PTY and Python subprocesses need actual tool compatibility evidence; scoped scratch avoids global cache writes, not infinite compatibility.
- A proxy can become an exfiltration authority unless domain/DNS, redirects, IP literals, UDP, socket families and local destinations are tightly mediated.
- cgroup v2 delegation is not universal; optional limits must report the absence and required limits fail closed.

## 11. Completion definition

The roadmap closes only when M001–M008 have accepted, exact-revision closure records demonstrating real Linux containment of filesystem read/write, Git metadata, host process/IPC and opted-in network/resource policy without unintended escalation; unqualified backends report limits, with explicit operator-controlled rollout. No title, tests-as-code, or claimed helper status alone constitutes closure.

## 12. Milestone status

M001's initial policy and native probes are implemented, but closure is blocked
on the authorized contained Git route described in its plan and closure record.
M002 is independently blocked on proposed ADR-0022 owner acceptance; M003–M008
retain their named dependency gates. No enhanced Linux isolation capability is
claimed as implemented or verified.
