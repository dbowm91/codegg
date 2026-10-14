# Linux Execution Isolation M003 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/linux-execution-isolation/003-protected-workspace-mounts.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `f18e50c1` (ADR-0027 accepted; M002 interface closure and hosted probes available)

Implementation commits or pull requests:

- None. M003 production implementation has not been admitted.
- Hosted capability evidence: GitHub Actions runs `38500815006` and `38501448851`.

## 1. Executive finding

The required protected writable workspace mount view is not implemented. A
direct writable workspace bind cannot safely mask an absent nested protected
path with Bubblewrap: preparing that destination creates the directory in the
host workspace before the target starts. OverlayFS, the tested alternative,
failed to mount on both the local Linux host and the Ubuntu hosted runner. No
production mount design is admitted until it can protect existing and future
Git/secret descendants without those host-side assembly effects.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Writable ordinary workspace with protected nested Git/secret paths | No production mount view implemented | not met | Legacy Landlock behavior is unchanged. |
| Mask absent future protected paths without host mutation | Local Bubblewrap direct-bind reproducer observed the missing mountpoint created in the host workspace | failed | `host_mountpoint_created=True`; violates the plan's no-side-effect and future-path requirements. |
| Overlay-backed protected view works on supported host | Local Bubblewrap OverlayFS attempt and hosted run `38500815006` | blocked | Both returned mount status 32 with `wrong fs type, bad option, bad superblock on overlay`. |
| Enhanced-tier host prerequisites are unprivileged and usable | Hosted run `38500815006` | partial | `sudo bwrap` namespace setup succeeded; non-root user-map setup failed. Privileged setup does not qualify the production no-root path. |
| Gitdir/worktree aliases, races and future nested paths are protected | No mount implementation or native adversarial fixture run | not run | Cannot infer this from Landlock path rules. |

## 3. Production implementation evidence

No M003 production code was added. Existing Landlock-only containment remains
the legacy path. The experiments did not promote Bubblewrap as a backend and did
not change defaults, Git routing or workspace synchronization.

## 4. Verification executed

### Commands and hosted evidence

- GitHub Actions Linux capability probe `38500815006` on Ubuntu 24.04.5,
  kernel `6.17.0-1022-azure`, x86_64, Landlock ABI 7. Namespace setup through
  privileged Bubblewrap succeeded; OverlayFS mount returned 32 and no upper
  file was written.
- GitHub Actions scope probe `38501448851` reconfirmed the ABI 7 hosted kernel
  and successful privileged user/PID/IPC namespace setup. It did not change
  the OverlayFS finding.
- Local Linux host: kernel `6.8.0-146-generic`, Landlock ABI 4. Bubblewrap
  rootless namespace setup was available; OverlayFS mount failed with the same
  status 32 result.
- Local direct-bind negative reproducer: binding a writable workspace and
  creating an absent nested mount destination with Bubblewrap left that
  directory in the host workspace before target execution.

No M003-specific product tests were run because no candidate implementation
passed the stop condition.

## 5. Invariant review

- No weaker or unverified mount policy was added.
- Existing Landlock launches and default profiles were left unchanged.
- Git metadata and secret descendants are not claimed protected by M003.
- M003 remains unavailable; no enhanced backend or capability report claims
  this mount view.

## 6. Failure and recovery review

The direct-bind experiment leaves a host-visible directory during setup. A
cleanup-after-failure strategy is insufficient: it can race with workspace
changes and still exposes a mutation before the child begins. OverlayFS did not
provide a usable alternative on the tested hosts. No mounts or persistent
temporary files were left by the failed experiments.

## 7. Migration and compatibility review

No schema, protocol, policy default or legacy launch behavior changed. A future
copy/staging alternative would need a separately reviewed, race-safe
reconciliation contract and would not be treated as the planned mount view
without evidence that leases, Git operations, path identity and rollback remain
correct.

## 8. Security review

The known setup-time workspace mutation is a high-severity correctness/security
finding for the proposed direct-bind construction. That construction was not
promoted. The tested rootless namespace path and OverlayFS capability results
are host-specific and do not justify a production fallback.

## 9. Documentation and operations

- M003 implementation plan now records the evidence blocker.
- Registry and subsystem roadmap disposition: blocked pending a side-effect-free
  mount construction method and native protected-path evidence.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Bubblewrap missing mountpoint setup mutates the writable host workspace; tested OverlayFS is unavailable | Cannot guarantee a protected writable view for existing and future descendants | Design a side-effect-free, race-safe mount or staging/synchronization architecture and prove it with native adversarial tests. |
| medium | Non-root namespace setup failed on the hosted runner | Hosted root-only namespace success does not qualify normal user launches | Qualify the actual non-root production path on supported distributions. |

## 11. Roadmap disposition

M003 is blocked; downstream work requiring protected mounts or full M003
capability cannot proceed. The negative reproducer is preserved as the stop
condition. Do not retry direct-bind masking or report the planned view as
enforced without a new design and evidence.

## 12. Registry updates

- Mark M003 blocked with this closure and its negative evidence.
- M004 remains independently eligible at its M003 interface boundary; its full
  process/IPC/seccomp capability still requires separate native proof.
- C002, M001 strict closure, and M008 remain blocked by their existing hard
  dependencies.
