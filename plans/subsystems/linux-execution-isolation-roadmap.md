# Linux Execution Isolation Roadmap

Status: active (command policy M001–M003 closed; M002 launch contract conditional; C004–C008 bounded slices closed; C006 closed negative for strict M003 acceptance; production M003–M008 remain unqualified)

**Independent CI test-contract note (2026-10-14):** `plans/subsystems/git-clean-authorization-ci-corrective-addendum.md` owns the stale `tests/git_network_integration.rs::clean_removes_untracked_files` expectation for unauthenticated destructive cleanup, and `plans/subsystems/git-reset-remote-test-contract-ci-corrective-addendum.md` owns the stale reset positives (`reset_hard_moves_head_and_drops_changes`, `reset_soft_keeps_working_tree` → typed `RequiresAdditionalAuthorization`) and the stale remote positives (`fetch/push/pull_from_local_bare_remote_succeeds` → typed `BackendUnavailable`, file remotes not yet qualified). All five are reconciled as typed-unavailable fixtures citing C002 (reset) / C003 (remote) as the positive-path owners. These are test-contract/CI correctives only: they do **not** unblock contained local Git C002, remote Git C003, or the M001 native backend, nor do they promote the accepted ADR-0031 candidate. Preserve `DestructiveRepositoryChange` as a typed authorization requirement.

Long-term references:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md`

Related ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016, **accepted ADR-0032** (syntax advisory, authority enforced), **accepted ADR-0031** (Linux backend mechanisms; production rollout remains gated). Related command-policy roadmap: `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Related but independent roadmap: `sandbox-containment-post-closure-corrective-roadmap.md` (C001–C003 closed; C004 conditionally closed); Windows sandbox stays separate.

## 1. Purpose and ownership boundary

Establish verifiable Linux untrusted-child execution containment in the Codegg daemon-owned local execution path. **Risk signals and command names are advisory under ADR-0032; the sandbox/lease/capability boundary, not a spelling filter, defines the executable authority.** Own Linux sandbox policy compilation, namespace/mount/network/IPC enforcement, trusted Git execution handoff safety, optional resource ceilings and obtained-capability reporting. Consume, but never replace, scheduler/ManagedProcessService process ownership, existing approval/lease authority, the Landlock ABI>=3 child helper, daemon-run Git services, and canonical audit. No agent-selected host/privilege policy.

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

### Next handoffs — explicit test-only vs production gates (2026-10-13)

- **C004 ADR identity corrective: closed at `plans/closure/linux-execution-isolation/012-status.md`.** `plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`. The Linux mechanism ADR is uniquely identified as ADR-0031; accepted command authority is ADR-0032. C004 preserved exact archived aliases without accepting the Linux design; ADR-0031 was accepted separately on 2026-10-14.
- **C005 native test-only Bubblewrap first hop: conditionally closed at `plans/closure/linux-execution-isolation/013-status.md`; positive namespace proof only, no production backend admission.** `plans/implementation/linux-execution-isolation/013-m003-bubblewrap-test-only-first-hop-and-feature-proof.md`. Real unprivileged user/mount namespace, helper identity, private status and no-target-on-setup-failure evidence; no production backend selection. Can run concurrently with C004.
- **C006 M003 mount backend strengthening: closed negative for strict M003 acceptance.** `plans/implementation/linux-execution-isolation/014-m003-protected-mount-view-and-git-metadata-backend.md`. ADR-0031 was accepted on 2026-10-14. The opt-in Bubblewrap candidate passed the reconciled production-source hosted native handoff (`38650935114` at `a28523f5a9bf3e89a03d21929d8616417eb1022f`); the trust-boundary review permits arbitrary nested `.git` as workspace content because trusted Git pins its selected worktree and gates executable routes. Linked worktree/common-dir aliases, races, and tool compatibility remain unqualified, so M003 stays blocked. Evidence is recorded at `plans/closure/linux-execution-isolation/014-status.md`.
- **C007 M004/M005 native denial oracle: conditionally closed at `plans/closure/linux-execution-isolation/015-status.md` for legacy-baseline effects and fixture design only; enhanced denial claims remain blocked.** `plans/implementation/linux-execution-isolation/015-m004-m005-process-ipc-offline-fixture-and-denial-matrix.md`. Host proc, signals, pathname/abstract Unix sockets, inherited FD, IPv4/IPv6 TCP/UDP/DNS and required no-userns failures; actual claims blocked until real M003/M004/M005 enforcement.
- **C008 M007 cgroup v2 feasibility: closed unavailable for the read-only host probe at `plans/closure/linux-execution-isolation/016-status.md`; production quotas remain blocked.** `plans/implementation/linux-execution-isolation/016-m007-resource-delegation-and-bounded-scheduler-feasibility.md`. This session has visible controllers but no writable delegation, `cgroup.kill`, or attach permission. No cgroup mutation was attempted; required resource guarantees still need a delegated native test host and pre-exec membership/limit/cleanup proof.
- **C009 M003 alias/race/compatibility corrective: active.** `plans/implementation/linux-execution-isolation/017-m003-alias-race-and-compatibility-corrective.md`. Exact-head hosted run `38671588961` on `ccbaa9bd` passed 2/2 native protected-mount tests on Ubuntu 24.04.5/kernel `6.17.0-1022-azure`/Bubblewrap `0.9.0`, including a linked worktree with an external read-only common Git directory and dependency-free `cargo check --offline`. M003 remains blocked on the rest of the alias matrix, setup-race boundaries, complete trusted-Git config attacks, rustup/Python/LSP/PTY/test compatibility, timings, cancellation, and concurrent-launch evidence. M002's interface is implemented; its macOS compile condition remains separately recorded. Preserve the negative C006 closure and Landlock default.
- **Command authorization M004 constrained tool routing: closed for its registered route-inventory and fail-closed admission scope.** `plans/implementation/command-authorization-and-execution-boundaries/007-m004-constrained-command-routing-and-tool-coverage.md`; closure `plans/closure/command-authorization-and-execution-boundaries/007-status.md`. Corrective M004-C1 plan 008 separately closes constrained Test/Python process-owner handoff. Constrained Git remains independently gated by C002/C003.
- **Git C002/C003 remain blocked** until qualified local filesystem/process/offline and separately mediated remote transport. Historical M001 closure and successful C001 typed Git policy remain unchanged. M002 Linux launcher is conditionally closed for internal interface/probes only. Command authorization M001–M003 remain strictly closed.

### Existing dependency graph

The M001 initial strict-Git implementation is **blocked** by legitimate command compatibility (9/12 historical Git mutation cases failed). **C001** (typed Git disposition, no fabricated spawn error) is **closed** under accepted ADR-0032; M001 Git closure is not a prerequisite for the Linux launch interface. **M002** has an implemented typed launch interface and probe under accepted ADR-0016/ADR-0032; its conditional closure retains a separate macOS cross-compile condition. ADR-0031 is accepted, while production promotion of Bubblewrap remains gated on exact-head native qualification. **M003** consumes the M002 interface and C001 Git-class interface; C009 owns the remaining linked-worktree/common-dir alias, replacement-race, trusted-Git configuration, and tool-compatibility evidence. M001's strict closure is not a hard dependency. **M004** requires M002 + accepted ADR-0031 and has M003 as an interface dependency. **M005** hard-depends on M004/M002, M006 on M005, and M007 on M002 (independent of M003–M006). **C002** (contained local Git recovery) requires C001 and native-verified M002–M005; **C003** (remote Git) requires C001, M005/M006, explicit credential/transport authority, and C002's state/lease interface. M001 can strictly close only after C001–C003 resolve its original workflows or an explicit scoped limitation is accepted; its historical closure is not rewritten. **M008** requires M001/C001–C003 and M002–M007. Qualified no-proxy or HTTPS-only subsets do not close the full roadmap. Native feature availability is an operational condition, never permission for uncontained fallback.

## 7. Milestones

### Registered successor packages

| Package | Status | Plan | Required proof |
|---|---|---|---|
| C004 | closed | `implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md` | Closure `../closure/linux-execution-isolation/012-status.md`; unique identities, exact archive provenance, guard and quick verification pass, no decision change |
| C005 | conditionally closed (test-only feasibility) | `implementation/linux-execution-isolation/013-m003-bubblewrap-test-only-first-hop-and-feature-proof.md` | Closure `../closure/linux-execution-isolation/013-status.md`; user/mount/PID/net/IPC child namespaces and no-target setup failure proved; production backend still blocked |
| C006 | closed negative (bounded native candidate qualification; strict M003 acceptance blocked) | `implementation/linux-execution-isolation/014-m003-protected-mount-view-and-git-metadata-backend.md` | Closure `../closure/linux-execution-isolation/014-status.md`; hosted Bubblewrap 0.9.0 ManagedProcessService slice passed. The M003 metadata trust boundary now permits arbitrary nested `.git` as workspace content while trusted Git binds the selected worktree and keeps executable routes gated; linked worktree, race and tool-compatibility matrix remains incomplete |
| C007 | conditionally closed (legacy baseline/design only) | `implementation/linux-execution-isolation/015-m004-m005-process-ipc-offline-fixture-and-denial-matrix.md` | Closure `../closure/linux-execution-isolation/015-status.md`; legacy host PID/loopback/abstract UDS effects recorded; enhanced denial still blocked |
| C008 | closed unavailable (test-only feasibility) | `implementation/linux-execution-isolation/016-m007-resource-delegation-and-bounded-scheduler-feasibility.md` | Closure `../closure/linux-execution-isolation/016-status.md`; no writable delegated subtree or attach permission on tested host; production quotas remain blocked |
| C009 | active corrective | `implementation/linux-execution-isolation/017-m003-alias-race-and-compatibility-corrective.md` | Hosted run `38671588961` on `ccbaa9bd` passes the linked-worktree/external-common-dir and offline Cargo-check slice (2/2). Remaining alias/race/config/tool/lifecycle evidence is required; no backend promotion by partial test-only success |



| Milestone | Class | Implementation | Status | User/operator exit |
|---|---|---|---|---|
| M001 Trusted Git execution boundary and hostile-metadata probes | invariant | `implementation/linux-execution-isolation/001-trusted-git-execution-boundary.md` | **blocked: C001–C003 corrective recovery** | Historical M001 closure preserves tested security improvement and 9/12 failing legitimate Git mutation cases; restore capability via actual typed/contained/transport authority, not string blockers |
| M002 Composable Linux launch and obtained-capability contract | infrastructure | `implementation/linux-execution-isolation/002-composable-linux-launch-contract.md` | **conditionally closed: interface/probe only** | Closure `../closure/linux-execution-isolation/002-status.md`; Linux and Windows GNU checks pass; macOS compile awaits SDK/sysroot. No concrete new backend; native qualification remains required |
| M003 Protected workspace mount view | capability | `implementation/linux-execution-isolation/003-protected-workspace-mounts.md` | blocked; C009 corrective active | ADR-0031 accepted; M002 typed interface implemented. C009 owns native alias/race, trusted-Git config, and tool compatibility evidence. M001 C001 is interface only; protected descendants and scoped workspace writes still require exact-head native proof. |
| M004 Process, IPC and seccomp containment | capability | `implementation/linux-execution-isolation/004-process-ipc-seccomp.md` | blocked: M002 + accepted ADR-0031; M003 interface | Host processes, daemon sockets and host proc are inaccessible under an opted-in isolated tier |
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

- ADR-0031 was accepted on 2026-10-14; qualifying native evidence is still required before M003 production backend promotion. M002 remains interface/probe-only.
- Bubblewrap relies on unprivileged user namespaces; locked-down hosts and WSL1 may reject it. Do not invent a bypass or use setuid wrapper; retain truthful legacy tier.
- Mount ordering/alias/overlays may conceal gitdir or allow indirect writes; require real security fixtures and refusal on ambiguity.
- Rust/Cargo builds, Git worktrees, LSP/PTY and Python subprocesses need actual tool compatibility evidence; scoped scratch avoids global cache writes, not infinite compatibility.
- A proxy can become an exfiltration authority unless domain/DNS, redirects, IP literals, UDP, socket families and local destinations are tightly mediated.
- cgroup v2 delegation is not universal; optional limits must report the absence and required limits fail closed.

## 11. Completion definition

The roadmap closes only when M001–M008 and C001–C003 have accepted, exact-revision closure records demonstrating real Linux containment of filesystem read/write, Git metadata, host process/IPC and opted-in network/resource policy without unintended escalation; unqualified backends report limits, with explicit operator-controlled rollout. No title, tests-as-code, or claimed helper status alone constitutes closure.

## 12. Milestone status

M001 historical Git closure is **blocked** (original legitimate Git mutation failures); C001 typed Git policy is **closed**, but C002/C003 remain gated on real contained local/remote execution. M002's launch-contract/interface slice is **conditionally closed** for the Linux legacy path; its macOS cross-compile condition remains recorded. C004 and C005 are closed; C006 is closed with a bounded negative candidate-qualification result. M003 no longer treats arbitrary future nested `.git` creation as a blocker when trusted Git is bound to its selected worktree. C009 is active to qualify linked-worktree/common-dir aliases, replacement races, trusted-Git config behavior, and tool compatibility. C007 is conditionally closed for its legacy baseline/design slice; C008 is closed unavailable for its read-only cgroup feasibility slice. ADR-0031 is accepted. Production M003–M008 remain gated on native containment. Command authorization M001–M003 are closed, with M004 corrective 008 active for constrained Test/Python execution. No enhanced Linux isolation enforcement is claimed.
