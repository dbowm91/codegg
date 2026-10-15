# M003 production admission readiness

Date: 2026-10-15
Scope: current Linux protected-mount candidate after reconciling the feature branch with current `origin/main`.

## Decision

**Do not admit the Bubblewrap protected-mount candidate as a production backend on any host configuration yet.** Keep it opt-in for qualification, keep Landlock canonical, and retain fail-closed behavior for required enhanced launches. This is an explicit no-admission decision, not a rejection of accepted ADR-0033.

The exact reconciled implementation now has hosted native protected-mount and trusted-Git evidence. That demonstrates candidate behavior, not a supported production profile: object-view timing varies materially between cold and cache-warm observations, general LSP/PTY routing is unqualified, and the protected-mount flag is not wired as a generally selectable production profile. The active admission plan now fixes measurable latency limits: private-view preparation warm p95 <=250 ms and cold p95 <=1,000 ms; paired end-to-end `git status` overhead warm p95 <=500 ms and cold p95 <=1,500 ms; no single view preparation above 3 seconds; and no more than 2x the paired baseline warm p95. Qualification uses at least 100 warm and 30 cold samples each for >=4,000-loose-object/100-MiB fragmented and content-equivalent packed stores. Results without cache control are unqualified.

## Observed host evidence

| Configuration | Evidence | Disposition |
|---|---|---|
| Ubuntu 24.04.5 hosted runner, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, UID/GID 1001, AppArmor `unconfined` | Exact reconciled implementation `c528ebff`; run [`38764899922`](https://github.com/dbowm91/codegg/actions/runs/38764899922): M004 3/3, protected-mount 11/11, trusted-Git 18/18. M004 includes proc-content invisibility, ptrace/process_vm denial and detached-session cancellation. Runner changed `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. | Candidate qualification only. The privileged ephemeral-runner policy change is not a general host-support claim. |
| Ubuntu x86_64 workstation, kernel `6.8.0-147-generic`, ext4, Bubblewrap `0.9.0`, UID/GID 1000, AppArmor active, `kernel.apparmor_restrict_unprivileged_userns=1`, `user.max_user_namespaces>0` | Local C009 protected-mount suite passed 11/11. M004 process-scope probe reports Landlock ABI 6 unavailable; required M004 target refused before execution. | M003 candidate only. The sysctl value alone does not mean namespace creation is denied; a direct unprivileged Bubblewrap probe succeeded here. No M004 positive host claim. |
| Ubuntu 24.04.5 hosted runner with `user.max_user_namespaces=0` | A new exact-head M004 workflow job disables user namespaces only in its ephemeral VM and asserts a required child leaves no target marker. Hosted evidence is pending. | Not admitted. This is an explicit disabled-userns failure-mode qualification, not a supported production profile. |
| Linux hosts without usable unprivileged user/mount namespaces, without a trusted Bubblewrap binary, or with an LSM policy denying namespace creation | Required-backend launch must fail before target execution; userns-disabled exact-host evidence is pending. | Unsupported for protected-mount requests; preserve legacy Landlock without relabeling it. |
| Other Linux distributions, kernels, LSM profiles and filesystems | No native evidence in this work. | Not admitted; support is feature-profile-specific and requires runtime probes plus native CI/host qualification, not distro-name inference. |

## Performance evidence

C009's recorded one-off measurement was 2.95 seconds for a 105 MiB, 3,945-file fragmented ext4 object store without reflink support. On the current workstation, a temporary test called the exact `GitEnvPolicy::apply_sync` object-view preparation path:

- Mixed checkout: 105 MiB total (`git count-objects`: 34.02 MiB loose, 71.33 MiB across 40 packs), one cache-warm sample: 325 ms.
- Single-pack clone after `git gc`: 29.07 MiB in one pack, one cache-warm sample: 47 ms.

These samples confirm that packing greatly reduces setup time on this host, but do not overturn the earlier cold 2.95-second result. The current data is too small to establish p95, cold-start cost, a production budget, or behavior on reflink-capable storage. No hard-link shortcut is acceptable because concurrent mutation would invalidate the immutable view.

An ignored, reproducible manual harness now collects repeated end-to-end samples through the same `GitEnvPolicy::apply_sync(["git", "status"], repository)` path. On 2026-10-15 it collected 20 samples against the current 109 MiB `.git/objects` store (4,100 loose objects, 40 packs; 36,696 KiB loose and 73,037 KiB packed) on the Ubuntu 6.8 workstation. Samples in milliseconds were 2210.215, 2080.726, 385.349, 367.736, 381.422, 407.571, 388.601, 386.260, 407.422, 387.672, 397.257, 381.508, 1209.466, 2080.381, 2198.292, 1996.022, 2036.532, 2148.848, 2000.558, and 2869.550. The median was 808.519 ms, nearest-rank sample p95 was 2210.215 ms, and maximum was 2869.550 ms. Invocation: `CODEGG_OBJECT_VIEW_BENCH_REPOSITORY=/home/sugarwookie/projects/codegg CODEGG_OBJECT_VIEW_BENCH_SAMPLES=20 cargo test -p egggit --lib private_object_view_warm_performance_samples --locked -- --ignored --nocapture`.

This is exploratory evidence, not an accepted latency qualification: the checkout is a mixed object store, the host was not isolated from background load, cache state was not controlled, and 20 samples cannot establish a stable production p95. The bimodal spread demonstrates that the earlier 325 ms single warm sample is not representative. It cannot be compared directly with the preparation-only threshold because it includes Git execution and the checkout's own state. No host profile is admitted; the active plan now defines a finite paired sampling protocol and explicit thresholds, so additional data must either meet those limits or preserve the no-admission decision.

### Controlled packed/fragmented follow-up (2026-10-15)

The repeated harness was extended to support an explicit warm pass and a
file-scoped cold pass using `posix_fadvise(POSIX_FADV_DONTNEED)` over each
object-store file. No global cache drop or host mutation was used. Both clones
contained the same 45,618 Git objects and ran paired preparation, protected
`git status`, and plain `git status` measurements. The packed fixture had one
27.49 MiB pack and no loose objects; the fragmented fixture had 45,618 loose
objects totaling 359.95 MiB and no packs. Each cell ran five repeated samples.

| Repository shape / cache | Private-view preparation ms | Protected status ms | Baseline status ms |
|---|---|---|---|
| Packed / warm | 488.377, 426.956, 550.686, 17.546, 507.132 | 563.383, 560.345, 536.129, 558.525, 574.542 | 11.868, 12.520, 11.647, 8.176, 12.158 |
| Packed / cold-advised | 678.229, 524.991, 545.210, 702.630, 623.493 | 706.368, 599.952, 556.243, 535.434, 610.084 | 11.277, 12.623, 8.469, 19.609, 12.149 |
| Fragmented / warm | 8802.986, 9007.093, 8011.941, 8904.007, 8564.619 | 9545.200, 9152.125, 9513.566, 9209.544, 7209.960 | 7.496, 7.598, 7.666, 7.433, 7.519 |
| Fragmented / cold-advised | 18685.705, 17837.412, 18775.857, 19196.370, 19433.033 | 9731.363, 8940.549, 9396.649, 8410.129, 9249.242 | 7.583, 7.893, 7.640, 7.451, 7.543 |

The five fragmented samples in each cache state all exceeded the plan's hard
3,000 ms maximum for a single preparation (warm: 8.01–9.01 s; cold-advised:
17.84–19.43 s). Protected fragmented status took 7.21–9.55 s against a
roughly 7–8 ms baseline. This is a decisive failure of the hard latency gate;
the larger 100-warm/30-cold distribution was stopped early rather than
spending more time on a candidate that had already breached its absolute cap
in all five repeats. Packed preparation also misses the 250 ms warm p95 budget
in four of five samples, and packed protected-status overhead exceeds the
500 ms warm budget in every sample. Packed cold observations remain below the
1,000/1,500 ms limits, but do not offset the warm failures.

**Performance disposition:** the current full-copy private object view is not
practical for routine agent Git reads on either tested shape, especially a
fragmented object store. C010's no-production-admission decision remains in
force. These measurements do not change Landlock selection, claim a supported
Bubblewrap host, or qualify a different snapshot mechanism. Any optimized
snapshot strategy requires a new security review and the same paired gate.

## Toolchain and interface gaps

- C009 qualified Cargo check/test, direct rustc, Python, shell, selected trusted Git reads, PTY first-hop, cancellation and concurrency.
- The M003-only filesystem view still omits `/proc/self/exe`; Rustup is not qualified in that mode. With M004's namespaced proc view, hosted `rustup toolchain list` and `rustup run stable rustc --version` passed on `c528ebff`. This is a positive toolchain fix for the M004 composition, not general Cargo/compiler or M003-only support.
- PTY policy association is implemented at the production daemon boundary: the
  transport-bound principal's stored profile is resolved, explicit constrained
  requests refuse before spawn, and the DTO cannot select a profile. FullHost
  behavior remains available when no constrained preference is selected. This
  is authority routing, not contained PTY evidence. Constrained LSP remains
  unavailable; its required owner is a separate daemon service keyed by
  immutable profile/workspace/server/generation and owning launch, restart,
  cancellation, and shutdown. The current launcher cannot preserve the required
  supervised JSON-RPC duplex channel through M004.
- `linux_launch::ProductionLinuxLaunchAdapter` still rejects process, network, seccomp and resource dimensions. The protected mount view is a candidate opt-in, not a generally admitted execution profile.
- M004 installs private proc after PID namespace setup, composes Landlock ABI 6 signal/abstract-socket scopes with PID/IPC namespaces, closes ambient descriptors, and installs the architecture-checked seccomp filter through Bubblewrap. Hosted denial/cancellation, proc-content/ptrace, detached-descendant and Rustup probes pass on the adjusted Ubuntu runner. Subsequent exact-head hosted evidence also passed concurrent namespace identity and Cargo check/managed-process probes. The remaining production gaps include contained PTY/LSP execution, broader supported-host qualification, and compiler/test workflow coverage. The internal M004 composition does not unshare the network namespace; M005 remains a separate authority and status dimension.
- M005 must separately express operator-owned `OfflineRequired`, prove TCP/UDP/DNS/host-local denial and inherited socket refusal, and retain explicit `LegacyUnrestricted` behavior. No agent text, profile alias, environment variable or automatic retry may widen network authority.
- Cgroup-v2 resource enforcement is independent and remains gated on actual writable delegation; it is not required to begin M004/M005.

## Required next actions

1. Keep hosted protected-mount/trusted-Git as candidate evidence; qualify exact supported feature profiles without changing their user-namespace policy. The restricted-userns CI job must first prove its test setup actually disables user namespaces.
2. Use the repeatable harness for paired packed/fragmented and controlled cold/warm distributions against the fixed thresholds above. Meeting the threshold clears only the performance gate, not general host admission or C002.
3. Expand M004 Cargo/compiler compatibility and supported-host qualification. Keep explicit constrained PTY refusal until the process owner can apply the qualified helper. Keep constrained LSP tools unavailable until a separately owned, supervised duplex service can prove launch, restart, cancellation, and shutdown under M004.
4. Keep M003 production admission disabled until all gates pass. Continue M004 process/IPC qualification, then implement and qualify operator-owned M005 `OfflineRequired`; local Git mutations (C002) follow actual M003–M005 containment, and remote mediation (C003) follows local Git recovery.

