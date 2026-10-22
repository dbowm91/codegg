# Linux Execution Isolation C008 — Closure Status

Status: closed (test-only feasibility; required production quotas remain blocked)

Source implementation plan:

- `plans/implementation/linux-execution-isolation/016-m007-resource-delegation-and-bounded-scheduler-feasibility.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `e17d3d95` (after C007 fixture closure).

Implementation commit:

- `a20aca38` — read-only Linux cgroup v2 delegation feasibility probe.

## 1. Executive finding

C008's bounded host-feasibility probe is closed with an **unavailable** result. This host exposes a unified cgroup v2 hierarchy and controllers, but the current user session has no writable delegated cgroup and cannot use `cgroup.kill`. The probe performed no cgroup writes, group creation, controller activation, process attachment, or quota changes. Production M007 resource limits remain blocked; no required budget is represented as enforced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Detect unified cgroup v2 and current placement | `linux_cgroup_v2_feasibility` opt-in integration test | pass | Root `/sys/fs/cgroup`; current cgroup `/user.slice/user-1000.slice/session-2.scope`; type `domain`. |
| Record available controllers and limits | Same test | pass | Root controllers: `cpuset cpu io memory hugetlb pids rdma misc`; current `cpu memory pids`; `memory.max=max`, `pids.max=max`, `cpu.max=max 100000`; `cgroup.subtree_control` empty. |
| Distinguish visibility from delegation and pre-exec attach | Read-only metadata and permission probes | unavailable | Opening `cgroup.procs` for write and `cgroup.kill` returned permission denied. No attach was attempted. |
| Avoid host/global mutation | Test code review | pass | The probe only reads cgroup metadata and checks whether write handles can be opened; it never writes, creates, enables, attaches, or kills. |
| Prove quotas and attach-before-exec | Requires writable delegated leaf | blocked | No disposable leaf or child attachment was possible or attempted. No quota, OOM, PID-limit, or CPU-throttling claim is made. |

## 3. Host evidence

The probe ran as the ordinary user in the current systemd session on Ubuntu 24.04.5 LTS, x86_64, Linux `6.8.0-146-generic`. It reported unified cgroup v2, current cgroup type `domain`, the controller and limit values above, `cgroup.kill=unavailable:permission denied`, and `attach_permission=unavailable:permission denied`. The exact opt-in command and output are preserved in the verification section below.

## 4. Verification executed

```bash
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target CODEGG_RUN_LINUX_CGROUP_V2_PROBE=1 cargo test --test linux_cgroup_v2_feasibility --locked -- --nocapture
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test -p codegg --lib managed_process --locked
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test managed_process_descendants --locked -- --test-threads=1
scripts/verify.sh quick
```

All commands passed. The opt-in probe passed 1/1; managed-process tests passed 20/20; managed-process descendant tests passed 5/5. Quick verification passed, including formatting, static guards, and workspace all-targets checking. Hosted CI was not run.

## 5. Invariant and security review

- No production execution policy, scheduler behavior, budget default, protocol, or backend selection changed.
- No cgroup resource dimension is reported as enforced from the presence of readable controllers.
- The probe did not write cgroup files or affect host processes, sessions, or limits.
- `ManagedProcessService` timeout, output, and process-tree behavior remain distinct from kernel-enforced CPU, memory, and PID quotas.

## 6. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No writable delegated subtree or attach permission is available to this session. | Required cgroup budgets and pre-exec membership cannot be established on this host. | Revisit only on an operator-delegated test host; prove attach-before-exec, limits, ownership, cleanup, cancellation, and recovery before production admission. |
| medium | No compatibility or workload benchmark was possible. | Toolchain and process-heavy workloads have no qualified resource profile. | Benchmark only after safe delegated execution is available. |

## 7. Roadmap and registry disposition

C008's read-only feasibility slice is closed with an unavailable-host result. Production M007 remains blocked on safe writable delegation, attach-before-exec, observed enforcement, and lifecycle proof. This work does not unblock another Linux production milestone. Command authorization M004 remains independently ready for bounded audit/test work.

The registry and Linux roadmap now link this closure and retain production M007 as blocked.
