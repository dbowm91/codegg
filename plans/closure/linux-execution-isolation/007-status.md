# Linux Execution Isolation M007 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/linux-execution-isolation/007-resource-envelopes.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `f18e50c1`

Implementation commits or pull requests:

- None. No cgroup-v2 resource-envelope implementation was admitted.
- Hosted capability evidence: GitHub Actions run `38501448851`.

## 1. Executive finding

The tested GitHub Actions Ubuntu runner exposes cgroup v2 but does not delegate
a writable subtree to the job. Its `/sys/fs/cgroup` mount is owned by root and
read-only. No CodeGG-owned cgroup attachment, per-attempt quota installation,
descendant membership verification, or cleanup implementation exists, so M007
cannot close.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Writable delegated cgroup-v2 parent | Hosted run `38501448851`: `/sys/fs/cgroup` reported `root root dr-xr-xr-x` | unavailable | The hosted job cannot create or configure an owned leaf. |
| CPU, memory, and pids ceilings installed before target execution | No production implementation | not met | Existing timeout/output limits are not cgroup quotas. |
| Target and descendants attached to an owned leaf | No production implementation or native test | not met | No membership-before-marker evidence exists. |
| Required limits refuse without delegation | No M007 launch integration | not run | Do not report unavailable policy as enforced. |
| Cleanup, cancellation, OOM and concurrent-job isolation | No owned cgroup lifecycle | not run | No PID/cgroup recovery behavior is claimed. |

## 3. Production implementation evidence

No M007 production code was added. The M002 host probe is diagnostic only and
does not create or attach a child to a cgroup. Existing launch profiles and
resource behavior are unchanged.

## 4. Verification executed

- GitHub Actions Linux capability probe `38501448851` completed on Ubuntu
  24.04.5, kernel `6.17.0-1022-azure`, x86_64. The cgroup v2 root was
  `root:root`, mode `dr-xr-xr-x`; it is not writable by the runner user.
- No M007 product test was run because there is no implementation to exercise
  and the required positive delegated host is unavailable.

## 5. Invariant review

- No host-wide controller setting was changed.
- No job was attached to an unrelated or reused cgroup.
- Existing limits are not represented as hard CPU, memory, or process-count
  quotas.
- No resource enforcement was reported as obtained.

## 6. Failure and recovery review

The capability probe is read-only. It created no cgroup and changed no host
settings. Cancellation, OOM recovery and stale-generation cleanup remain
unimplemented because there is no owned cgroup lifecycle.

## 7. Migration and compatibility review

No profile defaults, persistence, protocol or schema changed. The legacy
Landlock path remains unchanged and carries no new resource guarantee.

## 8. Security review

The critical missing condition is target-before-attachment prevention. Until a
trusted, attempt-owned leaf can be configured and verified before user exec,
required resource limits must remain unavailable. A read-only parent cannot be
treated as an optional successful limit.

## 9. Documentation and operations

- The implementation plan and subsystem roadmap identify the blocked
  delegated-cgroup requirement.
- An operator-delegated parent and a native supported host are required before
  a product implementation can be qualified.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No CodeGG-owned cgroup-v2 lease/attachment path and no writable delegated host | Descendants have no per-attempt CPU, memory or pids ceilings | Implement owned pre-exec attachment and lifecycle, then qualify on a writable delegated Linux host. |

## 11. Roadmap disposition

M007 is blocked. It is dependency-ready at the M002 interface but not
implemented or qualified. A hosted read-only cgroup root is a negative
capability result, not evidence that resource envelopes work.

## 12. Registry updates

- Keep M007 blocked pending an owned cgroup implementation and positive
  delegated-host evidence.
- No downstream milestone is unblocked.
