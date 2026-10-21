# Linux Execution Isolation M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/archive/implementation/linux-execution-isolation/004-process-ipc-seccomp.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#7-milestones`

Repository baseline reviewed: `6494912b3b158f52fa1eb3901b375bcbe1fcab50`

Implementation commits or pull requests:

- `170662a3e0ffbc6be09bb5d9ff26b856d5210a0d` — route constrained LSP through a daemon-owned M004 service scope.
- `9e6369683cf5ec6ed248ab6553865bdd7cba304c` — preserve constrained LSP ownership across platforms and fix cross-target compilation.
- `ae83d7989959f04e762b98b24222e9dd528fb7d5` — close C013 constrained PTY launch ownership.
- Exact hosted M004 source: `798f4762824751fcea75f3ba609da58ccf3a2f4b`.
- Exact-source hosted workflow: [run 39003759157](https://github.com/dbowm91/codegg/actions/runs/39003759157), success.
- Current-head M004 rerun: [run 39007099788](https://github.com/dbowm91/codegg/actions/runs/39007099788), pending at record preparation; production code matches the already-qualified source, with later commits limited to Git test diagnostics.
- Current-head macOS compile: [run 39007099308](https://github.com/dbowm91/codegg/actions/runs/39007099308), success.

## 1. Executive finding

M004's process, IPC, procfs, seccomp, PTY, and constrained LSP execution-owner
scope is complete. Every constrained production owner covered by this milestone
uses the daemon-resolved policy and the supervised M004 launch boundary; setup
failure remains fail-closed. Hosted Linux evidence proves host process and IPC
denial, ordinary managed child workflows, PTY routing and descendant cleanup,
and a daemon-owned constrained LSP duplex exchange with host-proc denial. A
restricted-user-namespace host refuses before target execution. FullHost keeps
its prior execution paths.

This closes the M004 implementation and qualification scope on the recorded
GitHub-hosted Linux configuration. It does not admit the general M003 protected
mount backend, claim M005 offline networking, qualify every Linux distribution
or LSM policy, or close C002/C003 Git recovery.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Hide host processes and proc contents; deny cross-boundary signals, ptrace/process_vm, abstract/pathname IPC, and unintended descriptors | Hosted run 39003759157, M004 managed-process target | pass | The exact-source process/IPC target passed 9/9, including concurrent namespace separation and cancellation. |
| Preserve ordinary managed process and compiler workflows inside the required tier | Hosted run 39003759157, Cargo/toolchain target | pass | Workspace checks, rustup/rustc and managed-process tests ran through the M004 child boundary. |
| Own process lifetime and clean up descendants on cancellation | Hosted run 39003759157 and C013 exact-source run 38983005904 | pass | Helper receipt, wait/reap, cancellation and PTY descendant cleanup are exercised. |
| Route daemon-authorized constrained PTY sessions through M004 without a caller-selected profile | C013 hosted run 38983005904; repeated in 39003759157 | pass | Principal-bound preference controls the profile; FullHost retains its existing route. |
| Route constrained LSP initial launch and restart through a workspace/profile-bound daemon owner | Hosted run 39003759157, constrained LSP duplex and procfs qualification | pass | JSON-RPC pipes and helper status ownership remain live through child exit; turn teardown shuts down the scope. |
| Refuse required confinement before target execution on restricted namespace hosts | Hosted run 39003759157, restricted-userns job | pass | No target marker or successful constrained admission is produced. |
| Preserve protected-mount and trusted-Git regression behavior | Hosted run 39003759157 | pass | Protected-mount tests passed 11/11 and trusted-Git tests 20/20; this does not promote M003. |
| Preserve cross-platform source compatibility | Hosted macOS compile run 39007099308 | pass | No Linux mechanism is claimed on macOS. |

## 3. Production implementation evidence

The Linux process tier composes private PID and IPC namespaces, a private proc
view, Landlock ABI 6 signal and abstract-socket scopes, a reviewed
architecture-checked seccomp filter, descriptor sanitation, and a setup
receipt/acknowledgment protocol. The target starts only after the helper proves
setup. Managed-process ownership handles status, cancellation, wait/reap and
namespace descendant cleanup.

Constrained PTY creation resolves the profile from the transport-bound
principal's daemon preference and routes through the supervised helper owner.
Constrained LSP turns receive a distinct service scope bound to the canonical
workspace and immutable daemon-resolved profile. The service-owned launcher
handles initial starts and restart generations; the constrained turn does not
borrow the shared FullHost LSP service. Tool admission is withheld if the
scope cannot be created or launched. Client requests cannot select or widen
the profile.

## 4. Verification executed

### Commands and hosted workflows

Hosted exact-source Linux workflow [39003759157](https://github.com/dbowm91/codegg/actions/runs/39003759157):

```text
M004 managed-process host PID and IPC denial probes
M004 constrained PTY live-owner qualification
M004 daemon-authorized PTY routing and descendant cleanup
M004 daemon-owned constrained LSP duplex and procfs qualification
Protected mount and Git regressions
Restricted-userns refusal-before-target checks
```

Hosted cross-platform workflow [39007099308](https://github.com/dbowm91/codegg/actions/runs/39007099308): macOS target compile succeeded.

The current-head M004 rerun [39007099788](https://github.com/dbowm91/codegg/actions/runs/39007099788) was still running during closure preparation. It is a repeat on the current head; the completed exact-source run above covers the M004 production implementation, and the intervening source changes are confined to unrelated Git test diagnostics.

### Results

- Process/IPC native target: 9 passed.
- Constrained PTY live owner: 1 passed.
- Daemon-authorized PTY routing and descendant cleanup: 1 passed.
- Constrained LSP duplex and procfs qualification: passed.
- Protected-mount regressions: 11 passed.
- Trusted-Git regressions: 20 passed.
- Restricted-userns refusal-before-target: passed.
- macOS target compile: passed.

The full workspace CI rerun [39007098374](https://github.com/dbowm91/codegg/actions/runs/39007098374) failed at `git_execution_origin_matrix::row_3_native_raw_git_subcommand`, returning typed `BackendUnavailable { capabilities: [RepositoryAlias] }` after 7,290 tests had passed and one failed. Earlier hosted attempts failed at structured Git object-view tests including `changed_files_returns_payload`, `log_oneline_format`, and `execute_log_returns_commits`. This remains a separate M003/Git object-view reliability issue and is not represented as a passing full-CI result here. The M004 hosted regression slice passed on the exact M004 source.

## 5. Invariant review

- No constrained target starts before required namespace, Landlock, procfs,
  descriptor, and seccomp setup succeeds: hosted positive and restricted-host
  refusal tests pass.
- Requested and obtained confinement remain distinct; Landlock-only and
  FullHost routes do not claim M004 enforcement.
- PTY and LSP profiles originate from daemon policy resolution, not client DTOs.
- Constrained LSP state is scoped to the turn, canonical workspace, profile,
  and service generation; restarts use the same owner.
- Helper status and child lifetime remain supervised through cancellation and
  exit; descendants are cleaned up.
- M004 process/IPC restrictions do not imply offline networking or cgroup
  resource enforcement.

## 6. Failure and recovery review

Required setup failure refuses before target execution. The restricted-userns
host proves this behavior. PTY and LSP setup failures do not fall back to direct
spawn; constrained LSP tools are unavailable when their owner cannot be
constructed. The helper receipt, wait/reap path, cancellation, PTY descendant
cleanup, and LSP service shutdown are exercised by hosted tests. Process and
namespace state is per launch; concurrent namespace separation is tested.

Daemon restart recovery of an already-running PTY/LSP session is not promised
by this milestone. Normal owner shutdown and child cancellation remain the
recovery boundary.

## 7. Migration and compatibility review

No schema or wire DTO migration was introduced. The client cannot request a
constrained profile; the daemon resolves it from trusted principal/session
policy. FullHost PTY and shared LSP behavior remain compatible. Constrained
Linux routes are opt-in and fail closed where required kernel or namespace
features are absent. macOS cross-compilation passed; this does not enable a
Linux backend on other platforms.

## 8. Security review

The authorization decision remains daemon-owned. The helper is fixed-location
and the required setup receipt precedes target exec. Host proc, cross-boundary
signals and process inspection, abstract and pathname IPC, and inherited
descriptor avenues are covered by native denial probes. Unsupported namespace
policy does not silently downgrade. PTY and LSP owners preserve process
supervision and cannot bypass the required helper for constrained requests.
Secrets and client-controlled command/profile fields are not added to the
protocol.

## 9. Documentation and operations

- Updated `architecture/security.md` with the constrained PTY and LSP owner
  contracts and their fail-closed behavior.
- Updated the subsystem roadmap, registry, and archived implementation plan
  with the closure disposition and evidence.
- Hosted qualification is tied to GitHub Actions runs 39003759157 and
  39007099308; exact runner and kernel facts are retained in workflow logs.
- Operationally, a host that denies unprivileged user namespaces or lacks the
  required Landlock ABI cannot run the required constrained tier. The route
  refuses before target execution.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | M004 host-policy evidence covers the recorded Ubuntu GitHub runner and a restricted-userns refusal fixture, not every supported Linux distribution, LSM configuration, or kernel combination. | Operators on other hosts may receive fail-closed refusal; no broader host admission is claimed. | Keep M004 mechanisms internal and qualify additional declared host configurations before any rollout claim. |
| medium | Full workspace CI has intermittent structured trusted-Git object-view failures outside the M004 native regression slice. | Branch-wide CI is not yet green; this may affect routine trusted Git operations and requires independent diagnosis. | Continue the M003/private-object-view reliability investigation; do not attribute it to or conceal it in this M004 closure. |
| medium | M003 protected-mount production admission remains denied by C010/C012 performance and compatibility gates. | M004 closure does not authorize selecting the protected-mount backend as a default or production profile. | Retain the no-admission decision until its own gates close. |
| medium | M005 offline networking and M007 cgroup resource limits are not provided by M004. | No offline-network or resource-quota guarantee follows from this record. | Qualify those dimensions under their own milestone plans. |

## 11. Roadmap disposition

M004 is closed for the implementation and qualification scope defined by its
plan. The constrained PTY and LSP production owners now use the M004 boundary,
and the hosted Linux evidence exercises their real launch paths. M005 may
proceed without the previous M004 production-owner dependency; it remains an
internal candidate until its own offline-network and owner-coverage gates pass.
M003 remains blocked by C010's no-admission decision. C002/C003 and M007 retain
their existing gates. This closure does not promote any enhanced Linux backend
for general production use.

## 12. Registry updates

- Mark M004 closed in `plans/registry.md` and
  `plans/subsystems/linux-execution-isolation-roadmap.md`.
- Move the completed implementation plan to
  `plans/archive/implementation/linux-execution-isolation/004-process-ipc-seccomp.md`.
- Keep M003 no-admission, M005 internal-only status, and C002/C003/M007
  dependencies unchanged.
