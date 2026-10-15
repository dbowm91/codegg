# M003 production admission readiness

Date: 2026-10-15
Scope: current Linux protected-mount candidate after reconciling the feature branch with current `origin/main`.

## Decision

**Do not admit the Bubblewrap protected-mount candidate as a production backend on any host configuration yet.** Keep it opt-in for qualification, keep Landlock canonical, and retain fail-closed behavior for required enhanced launches. This is an explicit no-admission decision, not a rejection of accepted ADR-0031.

The exact reconciled implementation now has hosted native protected-mount and trusted-Git evidence. That demonstrates candidate behavior, not a supported production profile: object-view timing varies materially between cold and cache-warm observations, there is no repeatable p95 or accepted latency budget, general LSP/PTY routing is unqualified, and the protected-mount flag is not wired as a generally selectable production profile.

## Observed host evidence

| Configuration | Evidence | Disposition |
|---|---|---|
| Ubuntu 24.04.5 hosted runner, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, UID/GID 1001, AppArmor `unconfined` | Exact reconciled implementation `f2e4bd76`; run [`38762601406`](https://github.com/dbowm91/codegg/actions/runs/38762601406): M004 3/3, protected-mount 11/11, trusted-Git 18/18. Runner changed `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. | Candidate qualification only. The privileged ephemeral-runner policy change is not a general host-support claim. |
| Ubuntu x86_64 workstation, kernel `6.8.0-147-generic`, ext4, Bubblewrap `0.9.0`, UID/GID 1000, AppArmor active, userns restriction sysctl 1 | After merge `611f6bee`, local C009 native protected-mount suite passed 11/11. | Candidate qualification only. Demonstrates the native tests can pass with this host's AppArmor userns restriction enabled. The exact hosted run exists but relaxed its ephemeral runner policy; repeatable performance and broader toolchain acceptance remain open. |
| Linux hosts without usable unprivileged user/mount namespaces, without a trusted Bubblewrap binary, or with an LSM policy denying namespace creation | Existing required-backend contract is fail-closed; no target is permitted on setup failure. | Unsupported for protected-mount requests; preserve the legacy Landlock mode without relabeling it. |
| Other Linux distributions, kernels, LSM profiles and filesystems | No native evidence in this work. | Not admitted; support must be based on runtime feature probe plus native CI/host qualification, not distro-name inference. |

## Performance evidence

C009's recorded one-off measurement was 2.95 seconds for a 105 MiB, 3,945-file fragmented ext4 object store without reflink support. On the current workstation, a temporary test called the exact `GitEnvPolicy::apply_sync` object-view preparation path:

- Mixed checkout: 105 MiB total (`git count-objects`: 34.02 MiB loose, 71.33 MiB across 40 packs), one cache-warm sample: 325 ms.
- Single-pack clone after `git gc`: 29.07 MiB in one pack, one cache-warm sample: 47 ms.

These samples confirm that packing greatly reduces setup time on this host, but do not overturn the earlier cold 2.95-second result. The current data is too small to establish p95, cold-start cost, a production budget, or behavior on reflink-capable storage. No hard-link shortcut is acceptable because concurrent mutation would invalidate the immutable view.

## Toolchain and interface gaps

- C009 qualified Cargo check/test, direct rustc, Python, shell, selected trusted Git reads, PTY first-hop, cancellation and concurrency.
- The M003-only filesystem view still omits `/proc/self/exe`; Rustup is not qualified in that mode. With M004's namespaced proc view, hosted `rustup toolchain list` and `rustup run stable rustc --version` passed on `f2e4bd76`. This is a positive toolchain fix for the M004 composition, not general Cargo/compiler or M003-only support.
- A general LSP child-path result and service-level PTY sandbox routing are not established.
- `linux_launch::ProductionLinuxLaunchAdapter` still rejects process, network, seccomp and resource dimensions. The protected mount view is a candidate opt-in, not a generally admitted execution profile.
- M004 now installs private proc after PID namespace setup, composes Landlock ABI 6 signal/abstract-socket scopes with PID/IPC namespaces, closes ambient descriptors, and installs the architecture-checked seccomp filter through Bubblewrap. Exact hosted denial/cancellation and Rustup probes pass on the adjusted Ubuntu runner. Broader proc-content/ptrace, detached descendants, concurrent-session and service-level PTY routing still need qualification. The internal M004 composition does not unshare the network namespace; M005 remains a separate authority and status dimension.
- M005 must separately express operator-owned `OfflineRequired`, prove TCP/UDP/DNS/host-local denial and inherited socket refusal, and retain explicit `LegacyUnrestricted` behavior. No agent text, profile alias, environment variable or automatic retry may widen network authority.
- Cgroup-v2 resource enforcement is independent and remains gated on actual writable delegation; it is not required to begin M004/M005.

## Required next actions

1. Keep the exact-head hosted protected-mount and trusted-Git result as candidate evidence; qualify another supported host profile without changing its user-namespace policy before any admission.
2. Add repeatable packed/fragmented performance sampling, include a cold-start method or explicitly leave cold behavior unqualified, and set a user-facing latency budget before any M003 admission.
3. Extend the M004 Rustup pass to Cargo checks/tests and compiler subprocess trees; qualify LSP and service-level PTY routing.
4. Keep M003 production admission disabled until those gates pass. Continue M004 process/IPC qualification, then implement and qualify operator-owned M005 `OfflineRequired`; local Git mutations (C002) follow actual M003–M005 containment, and remote mediation (C003) follows local Git recovery.

