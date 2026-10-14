# ADR-0027: Composable Linux Execution Isolation and Obtained-Capability Authority

Status: accepted

Date: 2026-10-14

Decision owners: project maintainers

Related specification sections:
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Affected subsystem roadmaps:
- `plans/subsystems/linux-execution-isolation-roadmap.md`

Related ADRs: ADR-0001 (execution authority), ADR-0002 (worktree isolation), ADR-0004 (sandbox and approvals), ADR-0016 (platform-owned launch). Complements these decisions; does not supersede or silently reinterpret them. The existing Windows ADR-0019, ADR-0020 and ADR-0021 numbers on `work/windows-native-sandbox-foundation` are reserved for that line.

## Context

At reviewed baseline `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`, the Linux helper applies Landlock ABI >=3 with no_new_privs in a child and truthfully reports filesystem-only containment, unrestricted network and inherited-FD limits. The managed process service owns launch/timeout/output/status, but has no mount/PID/IPC/network namespaces, seccomp program, or cgroup-v2 execution budget. Denied descendants under recursively writable Landlock grants cause a fail-closed launch; they cannot make `.git/config` read-only while retaining a writable parent. Git subprocesses in `egggit` and related trusted paths are not inherently covered by the Bash sandbox and can consume repo-controlled execution metadata.

Two distinct security problems exist: (1) protecting sandboxed code from host/workspace-sensitive resources, and (2) preventing sandboxed code from planting changes later executed by more privileged trusted services. Containing Bash alone does not prove a session-wide boundary. The network profile is currently unrestricted on all shell modes; switching the default would be a behavioral migration, not a cosmetic implementation choice.

## Decision drivers

- Preserve the working Landlock ABI>=3 filesystem boundary, helper authenticity, private status evidence, and fail-closed constrained launch.
- Preserve one daemon/scheduler/ManagedProcessService authority. No daemon self-sandboxing and no unreviewed tool-specific spawn owners.
- Protect denied descendants inside writable workspace roots, real worktree gitdir/common-dir targets, credentials, runtime IPC sockets and host process state.
- Distinguish actual filesystem-read, filesystem-write, network, process/IPC and resource guarantees; a capability probe alone never establishes applied isolation.
- Avoid a mandatory container daemon, privileged service, setuid bubblewrap dependency, external DNS proxy, or host firewall mutation for ordinary local development.
- Retain explicit operator authority and existing three sandbox profiles; no model-selected elevation, implicit network denial, or automatic fallback to FullHost.
- Support kernels/distributions where unprivileged user namespaces, cgroup delegation, or network namespaces are unavailable with exact capability diagnostics.

## Considered options

### A. Extend Landlock alone

Keep simple child-only enforcement, and optionally use Landlock TCP-port rights (ABI 4+) and abstract UNIX socket/signal scope (ABI 6+). Rejected as the sole strategy: Landlock lacks mount-view overlay and hostname/IP/UDP egress controls; denied descendants beneath a writable ancestor cannot be safely expressed with the existing write-grant model. May remain defense in depth or a measured compatibility backend.

### B. Compose Linux namespaces, mount policy, seccomp and existing Landlock (selected proposal)

Adopt a trusted bubblewrap-based launch adapter for a narrower filesystem view and namespace construction, with a tightly reviewed seccomp profile and optional Landlock as an additional restriction where its interaction has been proven. Keep Codegg-owned policy and process lifecycle; bubblewrap is a mechanism, not an external authority for approvals. This follows the well-qualified Linux sandbox pattern in Codex while preserving Codegg's tighter curated read roots. Use a trusted installed/bundled binary resolved independently of workspace/PATH input; never run untrusted or setuid helpers as an implicit privilege escalation.

### C. Containers/VMs for every local command

Stronger host/kernel boundaries are possible using rootless OCI containers, gVisor, or microVMs. Deferred as optional execution targets due to runtime/dependency, filesystem identity, build-cache, TTY and local-daemon integration overhead. May be appropriate for high-risk fully autonomous runs later.

## Decision

1. Define an internal **composable Linux launch policy** consumed by `ManagedProcessService` and applied before target code: filesystem-view requirements, mount exclusions, network intent, PID/IPC/proc requirements, syscall/FD constraints, and optional resource budget. Each dimension has `required/optional/not_requested` intent and **obtained** evidence; a required component failure aborts before user code. The existing Landlock-only backend continues to report its existing guarantees until an explicit rollout changes it.
2. Select a trusted bubblewrap namespace/mount adapter for the new Linux filesystem view. Build a read-allowlisted mount graph from Codegg's backend-neutral read roots (not `--ro-bind / /` unless every exposed subtree is explicitly proven safe), writable leased workspace/scratch mounts, and read-only or hidden overlays for protected descendants. Include `.git` directories, worktree `.git` pointer, resolved gitdir/common-dir, trusted executable metadata and sensitive agent settings. Reject symlink/rename/mount alias races or unsupported overlay ordering; do not rely on lexical string comparisons for grants.
3. The process policy must isolate host processes and IPC where claimed, including PID/IPC namespaces, a deliberate `/proc` view, socket mounts, session/TTY behavior, and a bounded seccomp baseline. `no_new_privs` is required; a parent-opened FD and same-UID host daemon socket are not automatically safe. Where these channels cannot be restricted, report the limit or reject the required profile.
4. Network intent must be **independent** of `SandboxProfile::{ReadOnly,WorkspaceWrite,FullHost}` and `ApprovalMode`. Introduce an explicitly operator-selected Linux network policy, initially `LegacyUnrestricted` (current behavior) or `OfflineRequired`. Add `ProxyAllowlist` only after a mediated TCP/DNS/UDP/AF_UNIX path is implemented and qualified. No network authority from shell command parsing, provider credentials, model-supplied env, tool text, or a child agent. Explicit network broadening must pass the existing approval/authority routing. Do not change default egress during infrastructure milestones.
5. The namespace helper must not expose daemon/SSH-agent/Docker/container-runtime/session-bus sockets, host `/proc`, secret-bearing inherited FDs or uncontrolled `/run` by mounting broad parents. Prefer sanitized descriptor inheritance and private status/spec channels. Required socket exceptions need typed trusted policy grants, not generic `/run` bind mounts.
6. The scheduler owns process limits (time, output, cancellation, and optional cgroup-v2/rlimit budgets) and cleanup. Resource containment is a separate obtained capability; no claim of memory/pids/CPU confinement from timeout or process-group cleanup alone.
7. A trusted Git operation remains a distinct authority boundary. The agent's ability to edit a workspace must not confer write authority to executable Git metadata or the ability to arrange automatic execution in trusted Git/daemon contexts. Explicit typed Git operations and approved configuration edits require a separate safe route; ordinary Bash must not silently regain host authority via Git.
8. Rollout is staged: first hostile-metadata safeguards, then internal launch contracts and native probes, then opt-in Linux hardening, then compatibility measurements and an explicit default-policy decision. Maintain exact user-facing requested/obtained facts, with versioning/migration if public DTOs or persisted preferences change. Existing macOS Seatbelt and Windows staged launch paths must remain unaffected.

## Consequences

### Positive
- Can protect children of writable workspaces without weakening secret exclusions.
- Provides a true network/process boundary on hosts where the full stack is obtainable.
- Supports evidence-driven capability tiers and optional future container/remote backends.

### Negative
- Adds a trusted Linux launch dependency and Linux-specific policy compilation and CI fixtures.
- Namespace, path alias, kernel and distribution constraints can cause previously working commands to fail under opt-in hardening.
- More complex process parentage, PID 1 behavior, stdio/PTY and signal forwarding must be tested.

### Neutral or deferred
- Full untrusted-kernel isolation requires a VM/microVM or additional kernel-hardening strategy.
- Trusted network proxy routing is deferred behind offline isolation; Landlock ABI 4 port rights alone are insufficient for a domain allowlist.
- No change to human-shell promotion semantics; human execution remains independent under ADR-0017.

## Compatibility and migration

Default legacy shell network remains unrestricted and its filesystem policy remains Landlock-only until explicitly qualified/migrated. No initial database migration is necessary. Any future persisted network preference or public enforcement DTO must be additive/versioned, preserve historical meaning, and be admitted through the operator's authority rather than agent requests. Never degrade `OfflineRequired` or protected-metadata isolation to host execution if user namespaces or bubblewrap are unavailable.

## Security and reliability implications

Independent verification is required for helper authenticity, path/FD identity, mount ordering, `/proc` and Unix socket exposure, process/signal isolation, proxy reachability, DNS resolution and network protocols, safe shutdown, child-reaping/cgroup teardown, and Git-mediated execution. A crafted workspace controls filenames, symlinks, Git configs and hooks and must be treated as adversarial. Group cleanup alone is not a cgroup limit. A child must never forge a successful confinement receipt.

## Verification

Native Linux real-child probes on supported kernels and at least one negative/no-user-namespace host; false-success injection; symlink, hardlink and moved-gitdir fixtures; offline TCP/UDP/Unix socket attempts; signal/`/proc` tests; trusted-Git hook/filter tests; timeout/cancellation; platform non-regression. Record kernel, Landlock ABI, namespace availability, seccomp and cgroup status for exact revision. Each milestone has separate closure evidence; no planned tests are asserted as executed.

References: https://github.com/openai/codex/blob/main/codex-rs/linux-sandbox/README.md ; https://github.com/containers/bubblewrap/blob/main/README.md ; https://docs.kernel.org/userspace-api/landlock.html ; https://git-scm.com/docs/git-config .


## Command syntax / policy layering (ADR-0028)

ADR-0028 is accepted and defines the cross-platform authorization semantics above this proposed Linux mechanism. `ManagedProcessService` must carry the same immutable capability ceiling whether a request originates in Bash, typed Git, a script, Python, or a native tool. This ADR's proposed Bubblewrap/seccomp implementation may not assume that a blocked Git subcommand, known command prefix or risk-classifier match is a security barrier. M002 can implement the backend-neutral internal launch/obtained-capability interface under already accepted ADR-0016/ADR-0028; adoption of the new concrete Linux backend remains subject to accepting this proposed ADR and native qualification. Git M001 C001/C002/C003 are parallel/consumer correctives, not a hard dependency cycle on M002.

## Supersession

None. ADR-0028 governs command authorization and representation-independent enforcement semantics; this ADR selects the Linux mechanisms implementing those semantics. Acceptance authorizes opt-in implementation and qualification of the staged Linux backend. It does not admit an enhanced backend, change default egress, or authorize a default-profile migration; each capability remains unavailable until its native closure evidence is accepted.
