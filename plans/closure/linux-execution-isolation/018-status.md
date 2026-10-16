# Linux Execution Isolation C010 — Closure Status

Status: conditionally closed (negative production-admission decision; M003 remains blocked)

Source implementation plan: `plans/implementation/linux-execution-isolation/018-m003-production-admission-and-host-qualification.md`
Source subsystem roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `f6befe51a111817a30ab692f4e7ad6166e323ce5`
Implementation commits:

- `611f6bee` — merge current `origin/main` into the Linux isolation branch.
- `f6befe51` — record C010 admission disposition, performance observations and M004/M005 sequencing.

## 1. Executive finding

C010 is conditionally closed with an explicit decision: **the Bubblewrap protected-mount candidate is not admitted for production on any Linux host configuration yet.** ADR-0033 remains accepted, Landlock remains canonical, and no production backend/default/profile routing changed.

The post-merge candidate passed exact-head native Linux qualification and cross-platform compile qualification. The performance evidence is contradictory across cold and cache-warm runs: the prior cold fragmented-store operation took 2.95 seconds, while this workstation measured 325 ms on a cache-warm mixed 105 MiB object store and 47 ms for a cache-warm 29 MiB packed store. This does not establish a cold/p95 budget. Rustup still requires `/proc/self/exe`, and production LSP/PTY routing has not been qualified. These findings make no-admission the only evidence-backed disposition.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reconcile feature branch against current main | Merge commit `611f6bee`; quick verification on merge and final source | pass | Resolved conflicts using current-main Git behavior and test contracts; preserved Linux isolation implementation and accepted ADR. |
| Accepted ADR and unique canonical identities | `python3 scripts/check_adr_integrity.py` | pass | ADR-0033 remains accepted; ADR-0032 remains accepted; active IDs unique and local paths resolve. |
| Exact-head Linux protected-mount native behavior | Hosted run [`38749191367`](https://github.com/dbowm91/codegg/actions/runs/38749191367), exact SHA `f6befe51` | pass | 11/11 protected-mount tests and 18/18 trusted-Git tests. Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap 0.9.0, UID/GID 1001, AppArmor `unconfined`. The ephemeral runner changed `kernel.apparmor_restrict_unprivileged_userns` 1→0. |
| Native behavior on an AppArmor-restricted-userns local host | Local exact merge-source native test | pass | Ubuntu x86_64, kernel `6.8.0-147-generic`, ext4, Bubblewrap 0.9.0, UID/GID 1000, AppArmor active, userns restriction sysctl remained 1; 11/11 tests passed. |
| Packed and mixed/fragmented object-store preparation | Temporary test called the exact `GitEnvPolicy::apply_sync` private-view path | partial | Cache-warm single samples: mixed 105 MiB store 325 ms; single-pack 29.07 MiB store 47 ms. C009 cold fragmented measurement was 2.95 seconds. No cold/p95 distribution or stable acceptable budget. |
| Windows cross-target compatibility after main merge | `CARGO_BUILD_JOBS=2 cargo check -p codegg --lib --target x86_64-pc-windows-gnu --locked` | pass | Zero errors; 36 platform/dead-code warnings. |
| macOS cross-target compatibility after main merge | Local check plus hosted run [`38750100690`](https://github.com/dbowm91/codegg/actions/runs/38750100690), exact SHA `f6befe51` | pass hosted | Local Linux-hosted attempt failed because the installed C compiler lacks Apple `-arch` and SDK support. Hosted macOS x86_64 target compile succeeded. |
| Existing sandbox regressions | Local `sandbox_containment`, `sandbox_landlock`, and `linux_bubblewrap_first_hop` targets | pass | 15 containment, 3 Landlock, and 1 first-hop namespace test passed. |
| Tool compatibility | C009 exact-head hosted and local evidence | partial | Cargo check/test, direct rustc, Python, shell, PTY first-hop and selected trusted Git reads passed. Rustup fails because `/proc/self/exe` is absent; LSP and production PTY profile routing remain unqualified. |
| Production admission by host configuration | Local Ubuntu host and hosted Ubuntu runner evidence above | no admission | Both prove candidate feasibility only. The hosted runner required a privileged AppArmor sysctl relaxation; local performance and toolchain gates remain. No distribution/profile is certified. |

## 3. Production implementation evidence

No production code, persisted policy, protocol, backend registration or default changed in C010. The feature branch was reconciled with current `origin/main`; current main's fail-closed Git test contracts were retained. The opt-in mount candidate remains unregistered for general production routing.

## 4. Verification executed

### Local

- `scripts/verify.sh quick` — passed on merge commit `611f6bee`.
- `python3 scripts/check_adr_integrity.py` — passed on final C010 tree.
- `python3 scripts/check_sandbox_contract.py` — passed.
- `python3 scripts/check_sandbox_policy_wiring.py` — passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- `python3 scripts/check_git_forbidden_patterns.py` — passed.
- `cargo test -p egggit --locked -- --test-threads=1` — 105 passed.
- `CODEGG_RUN_C006_BWRAP_NATIVE=1 CODEGG_RUN_C009_BWRAP_NATIVE=1 cargo test --locked --lib protected_mount_ -- --nocapture --test-threads=1` — 11 passed.
- `cargo test --test sandbox_containment -- --test-threads=1` — 15 passed.
- `cargo test --test sandbox_landlock -- --test-threads=1` — 3 passed.
- `CODEGG_RUN_LINUX_BWRAP_FIRST_HOP=1 cargo test --test linux_bubblewrap_first_hop -- --nocapture` — 1 passed.
- Windows GNU library check — 0 errors, 36 warnings.
- Local macOS cross-check — blocked by missing Apple compiler/SDK; hosted exact-head check passed instead.

### Hosted

- Linux run `38749191367` passed protected-mount 11/11 and trusted-Git 18/18 on exact SHA `f6befe51`.
- macOS run `38750100690` passed the x86_64 CodeGG target compile on exact SHA `f6befe51`.

## 5. Invariant review

- ADR-0033 remains accepted and unmodified; ADR-0032 remains accepted and unmodified.
- Canonical ADR identifiers remain unique.
- Landlock remains the default; the candidate remains explicit and unregistered for production.
- No test or feature result is used to imply process, IPC, seccomp, network or resource containment beyond the achieved filesystem view.
- Required enhanced execution remains fail-closed when trusted Bubblewrap or user/mount namespace setup is unavailable.

## 6. Failure and recovery review

The protected-mount native suite still exercises setup failure, cancellation, concurrency, replacement races and target gating. Unsupported runtime/userns setup does not trigger an automatic legacy launch under the enhanced label. No persistent state or cleanup protocol changed.

## 7. Migration and compatibility review

No migration or public compatibility change occurred. Existing Landlock execution is unchanged. The candidate is not enabled for a user profile or default. Rustup's `/proc/self/exe` requirement remains an M004 private-proc compatibility gate. LSP and production PTY routing remain unqualified.

## 8. Security review

The C009 trusted Git read boundary and object snapshot behavior remain intact through the merge. Candidate filesystem evidence does not imply process/IPC isolation, offline networking, seccomp, cgroup quotas, safe object-writing Git mutations, or mediated remote Git. The hosted runner's sysctl relaxation is limited to its ephemeral VM and is not treated as a supported operator configuration.

## 9. Documentation and operations

Added `research/linux-execution-isolation/m003-production-admission-readiness.md`; updated the Linux roadmap and registry; activated M004 against the stable M002/M003 internal interfaces; retained M005 blocked on M004. Cgroup-v2 enforcement remains independently blocked on writable delegation. Remote Git remains sequenced after contained local Git.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Cold/p95 private object-view cost is unknown; one cold fragmented sample took 2.95 seconds, versus 325 ms warm mixed and 47 ms warm packed samples. | No production latency budget can be asserted for repeated trusted Git reads. | Collect repeatable cold/warm packed and fragmented distributions and approve a user-facing budget before M003 admission. |
| medium | Rustup requires `/proc/self/exe`, absent from the M003-only view. | Common Rust toolchain selection still has a compatibility failure. | Resolve through M004's private proc view and native rustup tests. |
| medium | Object-writing Git routes, LSP and production PTY profile routing are not qualified. | C002 and M003 general production readiness remain incomplete. | C002 follows proven M003–M005 containment; complete production PTY/LSP route qualification within M004. |
| low | The hosted Ubuntu runner required a privileged ephemeral sysctl relaxation. | Hosted pass is not a general profile guarantee. | Keep host admission feature-probed and test locked-down failure behavior; do not require operator sysctl changes silently. |

## 11. Roadmap disposition

C010 closes as a negative production-admission decision. M003 remains blocked and no Linux host configuration is admitted for production. Landlock remains canonical. M004 is active because the internal M002 launch contract and M003 candidate interface are stable; M005 remains blocked on M004. M007 can proceed independently only on a host with delegated writable cgroup-v2 controls. C002 local Git recovery follows proven M003–M005 containment; C003 remote Git follows local Git recovery plus M005/M006 transport policy.

## 12. Registry updates and unblock audit

- C010 is conditionally closed in the roadmap and registry. The closure is recorded under recently closed work.
- M003 remains blocked because no Linux configuration met the production-admission criteria.
- M004 remains active against the stable M002/M003 internal interfaces. M005 remains blocked on M004's process/IPC contract and native evidence.
- C002/C003 and M007 retain their existing gates. C010 unblocked no registered implementation plan: local Git still requires M002–M005 native enforcement, remote Git still requires M005/M006 and C002's local state interface, and cgroup enforcement still requires a delegated writable cgroup-v2 host.

## Current-main integration follow-up (2026-10-15)

Reviewed integration baseline: `188edb371759f40b291052f25598bdb8ab57702d`,
including `origin/main` at `de49884f3fdc6d528620f2a2645694b9b19fc087`.

The later current-main merge revealed that ADR-0031 is already accepted for
authenticated idle continuation. The unchanged accepted Linux decision now has
canonical identity ADR-0033; ADR-0032 remains the accepted command-authority
decision. The Linux ADR integrity self-test now represents all three active
identities. This renumbering does not alter C010's no-admission result.

The active M003 plan now makes the performance gate finite and measurable:
100 warm and 30 cold paired samples for fragmented and equivalent packed stores;
private-view preparation p95 <=250 ms warm / <=1,000 ms cold; end-to-end
protected-view `git status` overhead p95 <=500 ms warm / <=1,500 ms cold; no
single preparation over 3 seconds; and warm p95 no greater than 2x baseline.
The historical 20-sample mixed-store data does not satisfy or refute these
limits because it did not isolate preparation or control cache state. No host
is admitted until the controlled gate passes along with the compatibility and
supported-host gates.

M004 has added an exact-host Cargo check/test probe and a separate ephemeral
runner job that disables user namespaces (`user.max_user_namespaces=0`) and
checks required-launch refusal before the target marker. Exact-head run
`38822273062` on `d010d728` passed the M004 native suite 8/8, protected-mount
regressions 11/11, trusted-Git regressions 18/18, and the separate restricted-
userns refusal job. A source audit confirms `InteractiveProcessService` and the `egglsp`
launcher currently bypass the M004 launch owner; PTY/LSP production routing and
protocol evidence remain hard M004 closure gates.

## M003 controlled object-view performance follow-up (2026-10-15)

The benchmark harness now supports repeated warm and cold-advised samples on
caller-supplied repositories. Two equivalent repositories were built from the
same 45,618 objects: one packed store (27.49 MiB, one pack) and one fragmented
store (359.95 MiB, all loose). Five samples per shape/cache condition were
collected on this host. This is an early-stop negative qualification, not a
100-warm/30-cold percentile qualification: all five fragmented preparations
breached the plan's hard 3,000 ms per-sample ceiling, so further sampling could
not make the candidate admissible.

| Shape/cache | Preparation (ms) | Protected `git status` (ms) | Plain status (ms) |
|---|---:|---:|---:|
| Packed/warm | 17.546–550.686 | 536.129–574.542 | 8.176–12.520 |
| Packed/cold-advised | 524.991–702.630 | 535.434–706.368 | 8.469–19.609 |
| Fragmented/warm | 8,011.941–9,007.093 | 7,209.960–9,545.200 | 7.433–7.666 |
| Fragmented/cold-advised | 17,837.412–19,433.033 | 8,410.129–9,731.363 | 7.451–7.893 |

Cold conditions use per-object `posix_fadvise(POSIX_FADV_DONTNEED)` and are
described as cold-advised rather than guaranteed cold; the host page cache was
not globally flushed. Packed warm preparation exceeded the 250 ms target in
four of five samples, and protected status overhead exceeded the 500 ms warm
limit in every sample. Fragmented preparation alone decisively exceeds the
hard cap. The private full-copy object view is therefore not practical for
routine agent Git reads on these repository shapes. C010's no-production-
admission decision stands; this evidence does not admit any host or change the
Landlock canonical backend. Any optimized view needs a fresh security review
and a new paired qualification.

## M004/M005 owner integration follow-up (2026-10-15)

PTY creation now resolves the authenticated transport principal's daemon-owned
profile. With no explicit constrained profile, the existing FullHost PTY path
is preserved. When ReadOnly or WorkspaceWrite is explicitly selected, PTY
creation fails before scheduler admission/spawn because no production M004
receipt is available. Preference lookup failure also refuses. The process
request does not accept a caller-selected profile. Focused PTY integration
coverage passes locally (20 tests).

Constrained turns cannot access/reuse the shared FullHost LSP service or its
preview/context adapters. LSP child ownership is assigned to a distinct
daemon-owned service keyed by immutable profile, workspace, server identity,
and generation, responsible for launch/restart/cancel/shutdown. The current
LSP launcher cannot retain supervised JSON-RPC duplex I/O through the M004
Bubblewrap helper, so constrained LSP remains unavailable before spawn. This
resolves the authority/ownership contract while retaining fail-closed runtime
behavior; it is not positive contained-LSP evidence or M004 closure.

## M005 candidate started (2026-10-15)

An internal-only candidate composes `--unshare-net` with the M004 private
process view. It is not persisted, user-selectable, or reported as enforced.
A compile assertion verifies the launch spec and an opt-in native test probes
host-loopback TCP denial; exact-head hosted execution passed on `d010d728` in
run `38822273062`. IPv6, UDP, DNS, inherited network descriptors, launch-owner
coverage, restricted-host network qualification, and offline toolchain
qualification remain open, as do M004 production-owner and host-admission
gates.

### Exact-head hosted qualification (2026-10-15)

The pushed implementation at `f3c2a73c13452ad398bee1e505e101ceb3e7c849`
passed Linux native workflow
[38817580703](https://github.com/dbowm91/codegg/actions/runs/38817580703)
on the ephemeral Ubuntu runner. The M004 process/IPC target passed 8/8,
including the M005 private-network namespace compile assertion and the native
host-loopback TCP denial probe. The workflow also passed protected-mount
regressions 11/11 and trusted-Git policy tests 18/18. Its separate restricted-
userns job proved the required M004 launch refuses before target execution.
The C006 hosted regression also passed at the same head in run
[38817580592](https://github.com/dbowm91/codegg/actions/runs/38817580592).

This is positive M004 candidate evidence and one M005 IPv4/TCP boundary
result. It does not establish full M004 production PTY/LSP integration,
supported-host admission, or M005 IPv6/UDP/DNS/inherited-descriptor and
offline-toolchain coverage. No enforcement status or production default is
promoted by these runs.

## Exact-head M004/M005 candidate qualification (2026-10-16)

Reviewed implementation head: `b30ebdcd53f5ac3a411e3365fe7288ceff1a4acd`.

Hosted Linux workflow
[38829815162](https://github.com/dbowm91/codegg/actions/runs/38829815162)
passed on Ubuntu 24.04, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, UID
1001, with the runner's `kernel.apparmor_restrict_unprivileged_userns=1`
relaxed to `0` only in that ephemeral VM for the positive namespace probes.
The M004 native target passed 8/8, including host proc/signal/IPC denial,
descendant cancellation, distinct concurrent PID/IPC namespaces, the M005
offline child probe, and a contained workspace Cargo check plus managed-
process tests. Protected-mount regressions passed 11/11 and trusted-Git
regressions 18/18. The separate restricted-userns job passed its required
launch refusal before target execution with `user.max_user_namespaces=0`.

The M005 child probe could not connect to host IPv4/IPv6 loopback TCP
listeners, deliver IPv4/IPv6 UDP datagrams to host listeners, resolve
`example.com`, connect to `198.51.100.1:443`, or use a deliberately inherited
host TCP descriptor. These are candidate denial observations, not a full
network qualification: the test did not positively establish host-side DNS or
external reachability before asserting child failure, did not exercise DNS
rebinding, and did not test every production launch owner. No persisted
`OfflineRequired` authority or `NetworkEnforcement::Enforced` status is
introduced.

The first run on `43329069` exposed malformed indentation in the generated
Python probe. The test was corrected to use a raw multiline string and pushed
as `b30ebdcd`; the successful run above includes that correction.

M003 remains not admitted. Its controlled packed/fragmented object-view
measurements still breach the agreed performance thresholds. M004 remains
active because constrained PTY creation refuses before spawn and constrained
LSP has no supervised duplex process owner. The passing native candidate
matrix does not close those production-owner gates or certify a Linux host.

## M005 network fixture positive controls (2026-10-16)

Commit `454e7a0f9abfb83cd39ee589fd13fd597a0ff3f1` added host-side positive
controls to `offline_network_blocks_host_network_and_inherited_socket_access`
before launching its offline candidate. The fixture now proves IPv4 and IPv6
loopback TCP listeners accept host connections, IPv4 and IPv6 UDP listeners
receive a host datagram, host DNS resolves `github.com`, and the host can
establish outbound TCP to `github.com:443`. This prevents a disconnected host,
broken fixture, or unavailable IPv6 loopback from being counted as a successful
offline denial.

At this exact code head, Linux M004 workflow
[38839130216](https://github.com/dbowm91/codegg/actions/runs/38839130216)
passed its `M004 managed-process host PID and IPC denial probes` step, which
also runs the M005 candidate denial probe. The separate restricted-userns job
passed its refusal-before-target check. Protected-mount/Git regressions and
full CI were still running when this record was written; their outcomes are
not inferred here.

This closes the host-reachability false-pass gap for the current IPv4/IPv6
TCP/UDP, DNS, external TCP, and inherited-descriptor observations. It does not
test DNS rebinding, exercise every production launch owner, or grant
`OfflineRequired` authority. M005 remains an internal candidate, and M004's
constrained PTY and supervised duplex LSP production-owner gates remain open.
