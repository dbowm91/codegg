# Sandbox Containment Audit Corrective Milestone C002 — Linux Default-Tier Containment Policy

Status: conditionally closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c002--linux-default-tier-containment-policy`

Long-term requirements:

- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs:

- `plans/adrs/ADR-0046-composable-linux-execution-isolation.md` (accepted; §5 and §8 are the authority for this milestone)
- `plans/adrs/ADR-0063-default-linux-containment-policy.md` (accepted 2026-11-10 — Option B)
- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: invariant

## 1. Objective

Apply Landlock ABI 6 scopes on the default Linux route and narrow the default
read-grant set so a contained command can no longer read a same-uid process's
environment through `/proc` or reach a host service socket through a broad
`/var/run` grant — without requiring a namespace, a container runtime, or a
privileged helper.

## 2. Why this milestone is ready

Its one hard dependency is satisfied. `ADR-0063` Option B was accepted on
2026-11-10 by the project maintainer. ADR-0046 §8 reserves the default
containment policy to "compatibility measurements and an explicit default-policy
decision", and §54 says the Landlock-only backend "continues to report its
existing guarantees until an explicit rollout changes it". That decision now
exists, so this plan may implement it.

Everything else is stable:

- `landlock::apply_process_scopes()` already exists, is already exercised by the
  mount-view tier, and returns the effective ABI.
- `policy::tool_read_allowances()` is a single function with no other caller that
  needs the broad set.
- The toolchain probes that must keep passing already exist
  (`a_sandboxed_shell_can_still_read_the_system_and_resolve_a_command`, the
  Cargo metadata probe).

ADR-0063 is accepted, so this plan is `ready for handoff` without further
authoring; the work packages below are already ordered against the decision's
clauses.

## 3. Current implementation evidence

**The default route is filesystem-only.**
`sandbox_config_for_profile_with_roots` enables no Linux opt-in for `ReadOnly` or
`WorkspaceWrite`, so `ProductionLinuxLaunchAdapter::plan` returns
`LinuxLaunchPlan::LegacyPrivateHelper`. The helper then calls `apply_backend`,
which selects Landlock and applies a filesystem ruleset. No PID namespace, no IPC
namespace, no network namespace, no private procfs, no seccomp, no descriptor
hygiene, and no Landlock `Scope`.

**ABI 6 scoping needs no namespace.**
`landlock::apply_process_scopes()` builds a second ruleset with
`Scope::from_all(ABI::V6)` under `CompatLevel::HardRequirement`, calls
`restrict_self()`, verifies `no_new_privs`, and returns the effective ABI. It is
called from exactly one place: `exec_linux_protected_mount_view`, i.e. only when
`linux_protected_mount_view` is already set. `probe_process_scopes()` exists and
is consulted only by the planner's protected-process-isolation branch.

**The default grant set includes broad runtime roots.**
`tool_read_allowances()` returns `/usr`, `/bin`, `/sbin`, `/lib`, `/lib64`,
`/opt`, `/etc`, `/dev`, `/proc`, `/sys`, `/var/run`, `/private/etc`,
`/private/var/db/timezone`, plus `~/.cargo/{bin,registry,git}` and the Rustup
toolchain roots. `launch_spec` filters to existing paths and adds them to
`read_paths`; `landlock::apply` grants `AccessFs::from_read(ABI::V9)` on each.

**Two exposures follow.**
`/proc` plus a shared uid makes `/proc/<daemon-pid>/environ` readable — the same
class of material the credential deny list protects by path. `/var/run` plus the
fact that Landlock does not mediate `connect(2)` on a **pathname** AF_UNIX socket
leaves session-bus, agent and container-runtime sockets reachable whenever the
socket mode admits the target uid. The ABI 6 scope covers **abstract** sockets
only.

**The tight tier already knows which roots to drop.**
`linux_mount_view::is_host_runtime_root` names exactly `/dev`, `/proc`, `/sys`,
`/run`, `/var/run`, `/etc`, `/opt` as roots the mount view refuses to bind from a
host parent, and re-adds a minimal `/dev`, a fresh `/tmp`, and eight explicit
`/etc` files. The knowledge exists; only the default tier does not use it.

**ADR-0046 §5 states the target.**
"The namespace helper must not expose daemon/SSH-agent/Docker/container-runtime/
session-bus sockets, host `/proc`, secret-bearing inherited FDs or uncontrolled
`/run` by mounting broad parents." That clause is scoped to the namespace helper;
this milestone extends its intent to the default backend under ADR-0063.

## 4. Invariants that must not regress

- A constrained request never degrades to host authority; an unavailable backend
  refuses.
- Landlock ABI 3 remains the hard floor; a host below it fails before target
  execution.
- `no_new_privs` is verified after `restrict_self()`, not assumed.
- Denies are carved out of grants, and a deny beneath a writable root is refused
  before launch.
- The toolchain still runs: `/etc` stays readable, name lookup resolves, and the
  shell resolves a command.
- `ReadOnly` still denies every persistent write and still permits device writes.
- No grant is widened to compensate for a narrowing.
- No milestone here promotes the Bubblewrap tier or claims network isolation.

## 5. Scope

### In scope

- `policy::tool_read_allowances()` — the Linux-relevant entries of the default
  read set.
- `landlock::apply` — accept and report the obtained scope ABI.
- The helper's Landlock dispatch — call `apply_process_scopes()` on the
  `LegacyPrivateHelper` route.
- `BackendEnforcement` / the descriptor — report the obtained scope and the socket
  and `/proc` posture.
- A committed toolchain matrix under `tests/` or `docs/`.

### Explicitly out of scope

- The Bubblewrap mount-view tier, its flags, and its production admission.
- Network isolation. `--unshare-net` stays opt-in and `NetworkEnforcement` is
  unchanged.
- Pathname AF_UNIX socket isolation. Only the mount view hides those.
- `SandboxProfile`, `ApprovalMode`, and approval routing.
- macOS and Windows.
- Descriptor hygiene and grant-path pinning (C006).
- Any new seccomp filter on the default route. ADR-0063 does not select it.

## 6. Required production changes

### Security and authorization

Apply `Scope::from_all(ABI::V6)` on the default Landlock route. Narrow the default
read set: replace the `/proc` grant with the narrowest grant the measured
toolchain needs, and replace `/var/run` with the specific files name lookup
requires. Each removal is justified by a committed measurement.

### Core/domain

`BackendEnforcement` gains obtained-scope facts for the Linux backend. Keep it an
in-memory value; do not serialize it to durable storage.

### Storage and migrations

None.

### Protocol and DTOs

None. The helper status frame is unchanged unless the obtained-scope fact must
cross the helper boundary, in which case `SANDBOX_STATUS_VERSION` is bumped and
the parent accepts both shapes for one release.

### Runtime and concurrency

The helper must fail closed if `apply_process_scopes()` fails on the default
route, exactly as the mount-view route already does: `Unavailable` before target
execution, never a contained claim.

### Frontend or operator surface

The descriptor text must distinguish "sockets unreachable" from "sockets not
isolated". Do not reuse the word `isolated` for a dimension the mechanism did not
obtain.

### Documentation and static guards

`architecture/security.md` records the default posture, the scope ABI, and the
named socket and `/proc` limits.

## 7. Ordered work packages

### Work package A — Measure before narrowing

Intent: the narrowing must be evidence-led, and the evidence must outlive the
milestone.

Required changes: build a committed matrix that runs, under the default contained
policy, at minimum: `/bin/sh -c true`; `git status` in a repository; `cargo
metadata`; a Python interpreter import; a Node interpreter import; and a name
lookup (`getent hosts` or equivalent). Run it three times: with the current grant
set, with `/proc` removed, and with `/var/run` removed. Record what fails and what
the failure names.

Acceptance evidence: the matrix is committed, each removal has a recorded
observation, and the residue (if any) is named per path rather than per tree.

### Work package B — Apply ABI 6 scopes on the default route

Intent: close the abstract-socket and signal channels at no namespace cost.

Required changes: call `apply_process_scopes()` in the helper's Landlock dispatch
for `LegacyPrivateHelper` as well as the protected route; fail closed on error;
capture the returned ABI instead of discarding it. Gate it on
`probe_process_scopes()` so a host below ABI 6 reports the scope as absent rather
than failing the launch, unless ADR-0063's accepted text requires a hard
requirement.

Acceptance evidence: a native fixture proves a contained child cannot signal a
same-uid process outside the scope and cannot reach an abstract socket bound
outside it, with a positive control; the descriptor reports the obtained ABI.

### Work package C — Narrow the `/proc` grant

Intent: close `/proc/<pid>/environ` for same-uid processes.

Required changes: replace the `/proc` grant with the narrowest grant the matrix
requires. Prefer a specific subtree or file set over the tree; `PathFd::new`
resolves `/proc/self` to the helper's own pid directory, and `exec` preserves the
pid, so a self-scoped grant is stable. Record the residue for anything that still
needs the host tree.

Acceptance evidence: a native fixture proves `/proc/<other-pid>/environ` is
unreadable under the default policy, with a positive control that reads it when
the grant is restored; the matrix passes.

### Work package D — Narrow the `/var/run` grant

Intent: stop exposing host service sockets through a broad runtime root.

Required changes: replace `/var/run` with the specific files name lookup requires
(for example the resolved `resolv.conf` path, which some distributions place under
`/run/systemd/resolve/`). Do not grant the directory.

Acceptance evidence: the matrix passes; a native fixture records that a pathname
AF_UNIX socket outside the grant is not reachable while an abstract socket is
scoped, and the descriptor states the pathname limit rather than implying
isolation.

### Work package E — Report the obtained posture

Intent: the operator must be able to tell what confined this launch.

Required changes: extend the Linux limits/guarantees so the descriptor names the
obtained scope ABI, the `/proc` posture and the pathname-socket posture, and does
not claim a dimension the mechanism did not obtain.

Acceptance evidence: a test fails if the descriptor claims socket isolation the
mechanism did not obtain.

## 8. Failure, cancellation, restart, and contention semantics

A failed scope application or a failed grant open aborts before target execution
and reports `Unavailable`/`SetupError`. There is no partial-contained state and no
fallback that re-adds a broad grant. The policy is rebuilt per launch, so there is
no restart or stale-generation state. Concurrent launches are independent.

## 9. Compatibility and migration

No persisted configuration or durable payload change. A host below Landlock ABI 6
runs the filesystem-only tier and reports the scope as absent. A host whose
toolchain genuinely needs the host `/proc` tree keeps a named residue rather than
a silent broad grant. No migration and no legacy path removal.

## 10. Required tests

### Focused unit tests

- The grant set no longer contains `/proc` or `/var/run` as trees, and every
  remaining broad root has a recorded justification.
- `apply_process_scopes()` failure is a refusal, not a warning.

### Integration tests

- Native Linux fixture: signal scoping, with a positive control.
- Native Linux fixture: abstract-socket scoping, with a positive control.
- Native Linux fixture: `/proc/<other-pid>/environ` unreadable, with a positive
  control.
- The committed toolchain matrix.

### Restart and recovery tests

Not applicable; no durable state.

### Contention and cancellation tests

Not applicable.

### Security and negative tests

- A defect-injection control restores a removed grant and observes the difference,
  so the denials are proved non-vacuous.
- The descriptor test fails on an unbacked claim.

### Migration and compatibility tests

- The matrix is the compatibility test; it must run in CI on a Linux runner, not
  only locally.

## 11. Required verification commands

```bash
# focused
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --lib security::sandbox::landlock

# real contained child (Linux host)
cargo test -p codegg --test security_permission -- sandbox_containment
cargo test -p codegg --test security_permission -- sandbox_landlock

# static guards
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test

# formatting and lint
cargo fmt
cargo clippy -p codegg --all-targets --locked -- -D warnings
```

The macOS host used to author this plan cannot produce any of the kernel evidence.
Do not claim it.

## 12. Documentation updates

- `architecture/security.md` — the default Linux posture, the obtained scope ABI,
  and the named `/proc` and pathname-socket limits.
- `plans/adrs/ADR-0063-default-linux-containment-policy.md` — status transition
  only; the decision text is immutable once accepted.
- The roadmap §12 status table and `plans/registry.md` on closure.

## 13. Acceptance criteria

1. A default (`LegacyPrivateHelper`) contained launch reports an obtained scope ABI
   and refuses when the scope cannot be applied, if ADR-0063 requires it as a hard
   requirement.
2. A contained child cannot signal a same-uid process outside the scope, proved
   with a positive control.
3. A contained child cannot reach an abstract socket bound outside the scope,
   proved with a positive control.
4. A contained child cannot read `/proc/<other-pid>/environ` under the default
   policy, proved with a positive control.
5. `/proc` and `/var/run` are not granted as trees, and every remaining broad root
   names the measurement that requires it.
6. The committed toolchain matrix passes in CI on a Linux runner.
7. The descriptor does not claim socket isolation the mechanism did not obtain.

## 14. Stop conditions

Stop and report rather than improvise when:

- ADR-0063 is rejected or amended. Then this plan stays blocked and the exposure is
  disclosed in the descriptor only.
- the matrix fails with a narrowed grant and no narrower entry restores it. Record
  the residue as a named limit; **do not restore the broad grant**.
- the only workable `/proc` grant is the host tree.
- the scope application cannot be made fail-closed without breaking a host that
  lacks ABI 6.
- the work would require a namespace, a container runtime, or a privileged helper.
- the work would widen any grant.

## 15. Closure evidence required

- ADR-0063's accepted status and the exact clauses implemented.
- The committed matrix with the three runs and their outcomes.
- Native kernel evidence for signal, abstract-socket and `/proc` scoping, each with
  a positive control.
- The non-vacuity demonstration for each denial.
- The named residues and the reason each survives.
- The exact commands run, labelled by host, with the Linux CI run identified.
- The roadmap §12 and registry updates, including the unblock audit.

## 16. Handoff notes

- This plan and **C001** both edit `policy.rs` allowance/deny tables. Do not run
  them concurrently; C001 lands first.
- This plan and **C006** both rewrite the Landlock limits disclosure and touch the
  helper. C006 lands after this plan.
- `apply_process_scopes()` already returns the effective ABI. Do not add a second
  probe; capture the return value.
- Landlock rules are path-beneath and resolved at open time. `/proc/self` is
  resolved by the helper to its own pid directory, and `exec` preserves the pid, so
  a self-scoped grant is stable — but verify it in the fixture rather than assuming
  it.
- The eight explicit `/etc` files in `linux_mount_view` are a working reference for
  what name lookup actually needs. Reuse that list rather than inventing one.
- Do not use a wall-clock benchmark to justify a narrowing. The evidence is "the
  matrix passes", not "the matrix is fast".
