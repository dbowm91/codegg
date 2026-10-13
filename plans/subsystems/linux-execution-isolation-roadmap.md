# Linux Execution Isolation Roadmap

Status: active (ADR-0024 accepted; Linux mechanism ADR-0023 proposed; interface and typed Git corrective ready)

Long-term references:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md`

Related ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016, **accepted ADR-0024** (syntax advisory, authority enforced), proposed ADR-0023 (Linux backend mechanisms). Related command-policy roadmap: `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Related but independent roadmap: `sandbox-containment-post-closure-corrective-roadmap.md` (C001–C003 closed; C004 conditionally closed); Windows sandbox stays separate.

## 1. Purpose and ownership boundary

Establish verifiable Linux untrusted-child execution containment in the Codegg daemon-owned local execution path. **Risk signals and command names are advisory under ADR-0024; the sandbox/lease/capability boundary, not a spelling filter, defines the executable authority.** Own Linux sandbox policy compilation, namespace/mount/network/IPC enforcement, trusted Git execution handoff safety, optional resource ceilings and obtained-capability reporting. Consume, but never replace, scheduler/ManagedProcessService process ownership, existing approval/lease authority, the Landlock ABI>=3 child helper, daemon-run Git services, and canonical audit. No agent-selected host/privilege policy.

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

The M001 initial strict-Git implementation is **blocked** by legitimate command compatibility (9/12 historical Git mutation cases failed). **C001** (typed Git disposition, no fabricated spawn error) is **ready** using accepted ADR-0024 and may proceed in parallel with command-policy M001 once its written interface is stable. **M002** (internal composable launch contract) is **ready** under accepted ADR-0016 and ADR-0024; M001 Git closure is **not** a hard prerequisite. Only *production promotion of the new Bubblewrap/Linux namespace backend* requires acceptance of proposed ADR-0023. **M003** (protected mount view) hard-depends on M002 + accepted ADR-0023 and consumes C001's **interface** contract, not M001's strict closure. **M004** (process/IPC/seccomp) hard-depends on M002 + accepted ADR-0023 and has M003 as an **interface** dependency. **M005** (offline network) hard-depends on M004/M002, M006 (mediated egress) on M005, and M007 (cgroups) on M002 (independent of M003–M006). **C002** (legitimate executable *local* Git recovery) hard-depends on C001 and native-verified M002–M005; **C003** (remote Git authorization/transport) hard-depends on C001, M005 and M006 plus explicit credential/transport authority, and consumes C002 state as an interface. M001 can strictly close only after C001–C003 resolve its original required workflows, or a separate explicit scoped limitation is approved and recorded; no hidden change to original closure record. **M008** full qualification hard-depends on M001/C001–C003 and M002–M007. Qualified no-proxy or HTTPS-only subsets can be recorded independently but do not close the full roadmap. Native kernel feature availability is an **operational** closure condition, never permission for uncontained fallback.

## 7. Milestones

| Milestone | Class | Implementation | Status | User/operator exit |
|---|---|---|---|---|
| M001 Trusted Git execution boundary and hostile-metadata probes | invariant | `implementation/linux-execution-isolation/001-trusted-git-execution-boundary.md` | **blocked: C001–C003 corrective recovery** | Historical M001 closure preserves tested security improvement and 9/12 failing legitimate Git mutation cases; restore capability via actual typed/contained/transport authority, not string blockers |
| M002 Composable Linux launch and obtained-capability contract | infrastructure | `implementation/linux-execution-isolation/002-composable-linux-launch-contract.md` | **conditionally closed: interface/probe only** | Closure `../closure/linux-execution-isolation/002-status.md`; Linux and Windows GNU checks pass; macOS compile awaits SDK/sysroot. No concrete new backend; ADR-0023 acceptance and native evidence remain required |
| M003 Protected workspace mount view | capability | `implementation/linux-execution-isolation/003-protected-workspace-mounts.md` | blocked: M002, ADR-0023 | M001 C001 is interface only; mounted writable workspace with protected Git/secret descendants verified native |
| M004 Process, IPC and seccomp containment | capability | `implementation/linux-execution-isolation/004-process-ipc-seccomp.md` | blocked: M002 + accepted ADR-0023; M003 interface | Host processes, daemon sockets and host proc are inaccessible under an opted-in isolated tier |
| M005 Offline network isolation | capability | `implementation/linux-execution-isolation/005-offline-network-isolation.md` | blocked: M002/M004 | Opt-in offline-required denies TCP/UDP and named network escape channels without silent host fallback |
| M006 Mediated allowlisted egress | capability | `implementation/linux-execution-isolation/006-mediated-egress.md` | blocked: M005 | Approved targets only through trusted proxy/DNS routing; no direct socket, local service or UDS bypass |
| M007 Bounded Linux resource envelopes | capability | `implementation/linux-execution-isolation/007-resource-envelopes.md` | blocked: M002 | CPU/memory/pids limits, cleanup and truthful unavailable/degraded status where delegation is absent |
| C001 M001 typed Git policy/results corrective | invariant | `implementation/linux-execution-isolation/009-m001-typed-git-policy-reconciliation.md` | **closed** | Closure `../closure/linux-execution-isolation/009-status.md`; implementation `ae025ef3`; typed outcomes, trusted non-executable Git, no raw bypass. C002/C003 remain blocked on real enforcement |
| C002 M001 contained local Git corrective | capability | `implementation/linux-execution-isolation/010-m001-contained-local-git-recovery.md` | blocked: C001, M002–M005 | Restore local Git/filters only through actual mounted, process/IPC/offline containment with Git metadata lease |
| C003 M001 remote Git corrective | capability | `implementation/linux-execution-isolation/011-m001-authorized-remote-git-recovery.md` | blocked: C001, M005/M006 and transport policy | Independently mediate HTTPS/SSH protocols and credentials; no generic network or agent socket fallback |
| M008 Qualification, compatibility and rollout gate | invariant | `implementation/linux-execution-isolation/008-qualification-and-rollout.md` | blocked: M001/C001–C003, M002–M007, command-policy M003 | Exact-revision native security and syntax-equivalence evidence; documented tool compatibility and explicit adoption/rollback gate |

## 8. Cross-cutting requirements

Storage/migration: prefer internal additive launch configuration initially; do not change persistent profiles or defaults without explicit migration plan. Public protocol changes must be versioned and independently reviewed. Keep operator authority and immutable parent/child ceilings.

Security: no network/IPC capability claim from Landlock filesystem success, raw `/proc` path allowance, static Python analysis or host firewall assumptions. Landlock 4 TCP port control and ABI 6 signal/abstract UDS scopes are useful defense-in-depth only where supported; never a substitute for UDP isolation, unix pathname socket restrictions or hostname allowlisting. Harden mount and proc views with real inode and namespace checks; do not bind host daemon and agent sockets.

Concurrency/recovery: one owned process tree per attempt, scratch & namespace lifetime bounded to it, cgroup teardown and proxy lifecycle scoped to job/session leases. Kill descendants and retain failure evidence before cleanup; no cross-session scratch/FD sharing. Restart must reconcile only owned stale resources and avoid deleting another live attempt.

Observability: requested policy, obtained filesystem/network/process/resource dimensions, backend/tool version, Landlock ABI, restricted scope, unsupported kernel feature and refusal phase; no raw credentials, private proxy credentials, absolute secret paths or unredacted command payloads in ordinary logs.

Compatibility: run real `git`, `cargo`, `rustc`, Python, shell, tests, common LSP/test paths on supported Linux distributions. Address bubblewrap/userns AppArmor/SELinux restrictions and no-userns hosts with actionable refusal; ordinary Landlock-only remains supported but never mislabeled as enhanced. No unconditional full-ci-per-platform matrix.

## 9. Verification strategy

Use focused kernel-faithful child probes in `tests/sandbox_containment.rs` / `tests/sandbox_landlock.rs` plus new Linux-only adversarial targets. Distinguish static plan/compilation tests from native execution success; all claims require actual hostile-operation failure and permitted operations success. Add no-userns negative fixture, hardlink/symlink/path-replacement, real gitdir, inherited FD, host `/proc`/socket, loopback and remote TCP/UDP, proxy DNS rebinding, cancellation/restart, worker/sibling contention, memory/pids quota probes. A supported Linux host must report kernel/version, effective features, binary identity and actual obtained status.

## 10. Risks and decisions

- ADR-0023 **proposed**; operator must accept the composed Linux backend/network intent contract before M002 production promotion. Do not self-accept during code implementation.
- Bubblewrap relies on unprivileged user namespaces; locked-down hosts and WSL1 may reject it. Do not invent a bypass or use setuid wrapper; retain truthful legacy tier.
- Mount ordering/alias/overlays may conceal gitdir or allow indirect writes; require real security fixtures and refusal on ambiguity.
- Rust/Cargo builds, Git worktrees, LSP/PTY and Python subprocesses need actual tool compatibility evidence; scoped scratch avoids global cache writes, not infinite compatibility.
- A proxy can become an exfiltration authority unless domain/DNS, redirects, IP literals, UDP, socket families and local destinations are tightly mediated.
- cgroup v2 delegation is not universal; optional limits must report the absence and required limits fail closed.

## 11. Completion definition

The roadmap closes only when M001–M008 and C001–C003 have accepted, exact-revision closure records demonstrating real Linux containment of filesystem read/write, Git metadata, host process/IPC and opted-in network/resource policy without unintended escalation; unqualified backends report limits, with explicit operator-controlled rollout. No title, tests-as-code, or claimed helper status alone constitutes closure.

## 12. Milestone status

M001's initial policy and native probes are implemented but historical closure remains **blocked** (legitimate Git mutation tests fail). C001 typed Git policy reconciliation is active; M002 **internal launch contract** remains independently ready without a hard dependency on M001 closure. C002/C003 local/remote Git restoration and new Linux backend deployment remain blocked by the named native enforcement/authorization gates. ADR-0023 is proposed and must be accepted before real Bubblewrap backend promotion; ADR-0024 governs representation-independent command authorization now. M003–M008 remain dependency-gated. No enhanced Linux isolation capability is claimed as implemented or verified.
