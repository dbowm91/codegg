# M003 production admission readiness

Date: 2026-10-15
Scope: current Linux protected-mount candidate after reconciling the feature branch with current `origin/main`.

## Decision

**Do not admit the Bubblewrap protected-mount candidate as a production backend on any host configuration yet.** Keep it opt-in for qualification, keep Landlock canonical, and retain fail-closed behavior for required enhanced launches. This is an explicit no-admission decision, not a rejection of accepted ADR-0031.

The exact reconciled implementation now has hosted native protected-mount and trusted-Git evidence. That demonstrates candidate behavior, not a supported production profile: object-view timing varies materially between cold and cache-warm observations, there is no repeatable p95 or accepted latency budget, general LSP/PTY routing is unqualified, and the protected-mount flag is not wired as a generally selectable production profile.

## Observed host evidence

| Configuration | Evidence | Disposition |
|---|---|---|
| Ubuntu 24.04.5 hosted runner, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, UID/GID 1001, AppArmor `unconfined` | Exact reconciled implementation `c528ebff`; run [`38764899922`](https://github.com/dbowm91/codegg/actions/runs/38764899922): M004 3/3, protected-mount 11/11, trusted-Git 18/18. M004 includes proc-content invisibility, ptrace/process_vm denial and detached-session cancellation. Runner changed `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. | Candidate qualification only. The privileged ephemeral-runner policy change is not a general host-support claim. |
| Ubuntu x86_64 workstation, kernel `6.8.0-147-generic`, ext4, Bubblewrap `0.9.0`, UID/GID 1000, AppArmor active, userns restriction sysctl 1 | After merge `611f6bee`, local C009 native protected-mount suite passed 11/11. | Candidate qualification only. Demonstrates the native tests can pass with this host's AppArmor userns restriction enabled. The exact hosted run exists but relaxed its ephemeral runner policy; repeatable performance and broader toolchain acceptance remain open. |
| Linux hosts without usable unprivileged user/mount namespaces, without a trusted Bubblewrap binary, or with an LSM policy denying namespace creation | Existing required-backend contract is fail-closed; no target is permitted on setup failure. | Unsupported for protected-mount requests; preserve the legacy Landlock mode without relabeling it. |
| Other Linux distributions, kernels, LSM profiles and filesystems | No native evidence in this work. | Not admitted; support must be based on runtime feature probe plus native CI/host qualification, not distro-name inference. |

## Performance evidence

C009's recorded one-off measurement was 2.95 seconds for a 105 MiB, 3,945-file fragmented ext4 object store without reflink support. On the current workstation, a temporary test called the exact `GitEnvPolicy::apply_sync` object-view preparation path:

- Mixed checkout: 105 MiB total (`git count-objects`: 34.02 MiB loose, 71.33 MiB across 40 packs), one cache-warm sample: 325 ms.
- Single-pack clone after `git gc`: 29.07 MiB in one pack, one cache-warm sample: 47 ms.

These samples confirm that packing greatly reduces setup time on this host, but do not overturn the earlier cold 2.95-second result. The current data is too small to establish p95, cold-start cost, a production budget, or behavior on reflink-capable storage. No hard-link shortcut is acceptable because concurrent mutation would invalidate the immutable view.

An ignored, reproducible manual harness now collects repeated end-to-end samples through the same `GitEnvPolicy::apply_sync(["git", "status"], repository)` path. On 2026-10-15 it collected 20 samples against the current 109 MiB `.git/objects` store (4,100 loose objects, 40 packs; 36,696 KiB loose and 73,037 KiB packed) on the Ubuntu 6.8 workstation. Samples in milliseconds were 2210.215, 2080.726, 385.349, 367.736, 381.422, 407.571, 388.601, 386.260, 407.422, 387.672, 397.257, 381.508, 1209.466, 2080.381, 2198.292, 1996.022, 2036.532, 2148.848, 2000.558, and 2869.550. The median was 808.519 ms, nearest-rank sample p95 was 2210.215 ms, and maximum was 2869.550 ms. Invocation: `CODEGG_OBJECT_VIEW_BENCH_REPOSITORY=/home/sugarwookie/projects/codegg CODEGG_OBJECT_VIEW_BENCH_SAMPLES=20 cargo test -p egggit --lib private_object_view_warm_performance_samples --locked -- --ignored --nocapture`.

This is exploratory evidence, not an accepted latency qualification: the checkout is a mixed object store, the host was not isolated from background load, cache state was not controlled, and 20 samples cannot establish a stable production p95. The bimodal spread demonstrates that the earlier 325 ms single warm sample is not representative enough to set an admission budget. No host profile is admitted; production admission still needs a stated user-facing budget and controlled cold/warm runs on each candidate storage profile.

## Toolchain and interface gaps

- C009 qualified Cargo check/test, direct rustc, Python, shell, selected trusted Git reads, PTY first-hop, cancellation and concurrency.
- The M003-only filesystem view still omits `/proc/self/exe`; Rustup is not qualified in that mode. With M004's namespaced proc view, hosted `rustup toolchain list` and `rustup run stable rustc --version` passed on `c528ebff`. This is a positive toolchain fix for the M004 composition, not general Cargo/compiler or M003-only support.
- A general LSP child-path result and service-level PTY sandbox routing are not established.
- `linux_launch::ProductionLinuxLaunchAdapter` still rejects process, network, seccomp and resource dimensions. The protected mount view is a candidate opt-in, not a generally admitted execution profile.
- M004 installs private proc after PID namespace setup, composes Landlock ABI 6 signal/abstract-socket scopes with PID/IPC namespaces, closes ambient descriptors, and installs the architecture-checked seccomp filter through Bubblewrap. Hosted denial/cancellation, proc-content/ptrace, detached-descendant and Rustup probes pass on the adjusted Ubuntu runner. A separate concurrent-session namespace-identity test is awaiting exact-head hosted evidence. Broader Cargo/compiler use and service-level PTY routing remain open. The internal M004 composition does not unshare the network namespace; M005 remains a separate authority and status dimension.
- M005 must separately express operator-owned `OfflineRequired`, prove TCP/UDP/DNS/host-local denial and inherited socket refusal, and retain explicit `LegacyUnrestricted` behavior. No agent text, profile alias, environment variable or automatic retry may widen network authority.
- Cgroup-v2 resource enforcement is independent and remains gated on actual writable delegation; it is not required to begin M004/M005.

## Required next actions

1. Keep the exact-head hosted protected-mount and trusted-Git result as candidate evidence; qualify another supported host profile without changing its user-namespace policy before any admission.
2. Use the repeatable harness to collect controlled packed/fragmented and cold-start distributions, then set a user-facing latency budget before any M003 admission. The current 20-sample mixed-store distribution is high-variance exploratory evidence only.
3. Extend the M004 Rustup pass to Cargo checks/tests and compiler subprocess trees; qualify LSP and service-level PTY routing.
4. Keep M003 production admission disabled until those gates pass. Continue M004 process/IPC qualification, then implement and qualify operator-owned M005 `OfflineRequired`; local Git mutations (C002) follow actual M003–M005 containment, and remote mediation (C003) follows local Git recovery.

