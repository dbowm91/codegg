# ADR-0061: Default Linux Containment Policy and Host-Runtime-Root Exposure

Status: accepted — Option B selected 2026-11-10 by the project maintainer as the explicit default-policy decision ADR-0046 §8 reserves

Date: 2026-11-10

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Affected subsystem roadmaps:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md`
- `plans/subsystems/linux-execution-isolation-roadmap.md` (cross-reference only; this
  ADR does not promote its production backend)

Related ADRs: `ADR-0004`, `ADR-0016`, `ADR-0046`. This decision **operationalises**
ADR-0046 §5 and §8 for the backend that ADR-0046 left untouched; it does not
supersede or reinterpret ADR-0046's namespace adapter, its staged rollout, or its
obtained-capability authority.

## Context

ADR-0046 §5 states that the namespace helper "must not expose
daemon/SSH-agent/Docker/container-runtime/session-bus sockets, host `/proc`,
secret-bearing inherited FDs or uncontrolled `/run` by mounting broad parents".
That prohibition is scoped to the **namespace helper** — the opt-in Bubblewrap
mount-view tier. ADR-0046 §8 then requires "compatibility measurements and an
explicit default-policy decision" before the default changes, and §54 says the
"existing Landlock-only backend continues to report its existing guarantees until
an explicit rollout changes it".

That decision has never been taken. The consequence, measured at
`7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`:

- `policy::tool_read_allowances()` returns `/usr`, `/bin`, `/sbin`, `/lib`,
  `/lib64`, `/opt`, `/etc`, `/dev`, `/proc`, `/sys`, `/var/run`,
  `/private/etc`, `/private/var/db/timezone`, plus `~/.cargo/{bin,registry,git}`
  and the Rustup toolchain roots. Every contained Linux launch receives that set.
- `sandbox_config_for_profile_with_roots` enables **no** Linux opt-in for
  `ReadOnly` or `WorkspaceWrite`, so the default contained launch is
  `LinuxLaunchPlan::LegacyPrivateHelper`: Landlock filesystem rules only. No PID
  namespace, no IPC namespace, no network namespace, no private procfs, no
  seccomp, no descriptor hygiene, and no Landlock ABI 6 `Scope`.
- `landlock::apply_process_scopes()` — which needs no namespace at all, only a
  second `restrict_self()` with `Scope::from_all(ABI::V6)` — is called only from
  `exec_linux_protected_mount_view`, i.e. only when the protected mount view is
  already requested.

Two exposures follow directly, and neither is disclosed in the enforcement
descriptor the operator sees:

1. **Process environment.** `/proc` is granted `AccessFs::from_read`, and the
   target shares the daemon's uid, so `/proc/<daemon-pid>/environ` is readable.
   That is the daemon's environment, which is exactly the material the curated
   credential deny list exists to protect by path. The deny list protects
   `~/.config/codegg` and the provider credential store; `/proc` reaches the same
   class of secret by a different route.
2. **Host sockets.** `/var/run` is granted read. Landlock does not mediate
   `connect(2)` on a pathname AF_UNIX socket, and the ABI 6 scope covers
   **abstract** sockets only. `/run/user/<uid>/` (session bus, gpg-agent,
   ssh-agent) and a group-accessible container-runtime socket therefore remain
   reachable whenever the corresponding socket mode admits the target's uid.

This is a default-policy question, not a defect to be patched quietly, because
ADR-0046 §8 reserved it and because the obvious remedies have real
compatibility costs: `/proc` is read by ordinary toolchains, and `/run` is where
several distributions resolve DNS.

## Decision drivers

- A credential boundary that is enforced by path but bypassable by `/proc` is a
  partial boundary, and reporting it as complete is worse than reporting it as
  partial.
- ABI 6 `Scope` costs one `restrict_self()` on the same process and requires no
  namespace, no privileged helper and no container runtime, so its absence from
  the default tier is a wiring omission rather than a capability limit.
- Tool compatibility is load-bearing: `policy.rs` documents that "a sandbox that
  fails every command is not containment, it is denial", and the suite asserts
  that `/etc` stays readable and the toolchain still resolves.
- Narrowing must be evidence-led. Removing `/proc` because it is listed in
  ADR-0046 §5, without measuring what breaks, trades one silent failure for
  another.
- No default change may widen authority, and no target may acquire a broader
  profile than its parent.
- The operator must be able to tell "this backend does not isolate sockets" from
  "this backend isolates sockets and none were reachable".

## Considered options

### A. Keep the default grant set and disclose the exposure

Change nothing in the launch path; add the `/proc` and `/run` exposures to
`LANDLOCK_LIMITS` so the enforcement descriptor states them.

Benefits: zero compatibility risk; small, reviewable change; makes the operator
claim truthful. Costs: leaves a live credential path open on the default tier for
every contained command. Failure mode: the disclosure is correct and ignored, and
the next audit reports the same finding. This is the status quo plus honesty, not
a fix.

### B. Narrow the grant set and apply ABI 6 scopes unconditionally (selected)

Three separable changes, landed in one milestone but independently revertible:
(1) apply `Scope::from_all(ABI::V6)` on the Landlock route whether or not the
mount view is requested; (2) replace the `/proc` grant with the narrowest grant
the measured toolchain actually needs; (3) drop `/var/run` and re-add only the
specific files a name lookup requires. Report the obtained scope and the reduced
grant set in the enforcement descriptor.

Benefits: closes the `/proc/<pid>/environ` path and the abstract-socket channel
without a namespace, a container runtime, or a privileged helper; keeps the
curated-read-root design that already distinguishes Linux from macOS; the scopes
are additive restriction and cannot widen anything. Costs: `/proc` and `/run`
narrowing is compatibility-sensitive and must be measured per toolchain. Failure
mode: a narrower grant breaks a build, and the temptation is to restore the
broad grant silently — which is why the milestone carries a stop condition rather
than a fallback.

### C. Require the protected mount view for every contained Linux launch

Promote the Bubblewrap tier to the default: fresh tmpfs root, minimal mounts,
PID/IPC/network namespaces, seccomp, private procfs, descriptor hygiene.

Benefits: the strongest posture, and it satisfies ADR-0046 §5 literally. Costs:
it makes an unprivileged-user-namespace host and a trusted Bubblewrap install
mandatory for every contained command; ADR-0046 §8 deliberately staged this
behind opt-in and compatibility measurement, and ADR-0046 §3 rejects a mandatory
container runtime or privileged service for ordinary local development. Failure
mode: hosts without unprivileged user namespaces lose contained execution
entirely and fall back to refusal, which converts a confidentiality gap into an
availability regression. Not selected as the default; remains the opt-in tier and
the target end state.

## Decision

1. The default Linux containment policy is **Landlock filesystem rules plus
   Landlock ABI 6 `Scope`**, with no namespace requirement. `Scope` is applied on
   the Landlock route unconditionally, including
   `LinuxLaunchPlan::LegacyPrivateHelper`, and its effective ABI is recorded
   rather than discarded.
2. The default read-grant set is narrowed to the smallest set that a measured
   toolchain matrix requires. `/proc` is replaced by the narrowest grant that
   restores the matrix; `/var/run` is replaced by the specific files name lookup
   requires. **No broad `/proc` or `/run` grant survives without a recorded
   measurement naming what needs it.**
3. Narrowing is evidence-led and fail-closed. A path is removed only after the
   toolchain matrix passes without it. If the matrix fails and no narrower grant
   restores it, the residue is **recorded and reported**, not restored
   silently — the same rule this repository applies to derived theme tokens.
4. Pathname AF_UNIX sockets are explicitly **not** claimed as isolated by the
   default tier. The enforcement descriptor states the socket and `/proc`
   posture as an obtained fact, separately from filesystem and network, so no
   operator reads "contained" as "sockets unreachable".
5. This decision does **not** promote the Bubblewrap tier, does **not** change
   `SandboxProfile::{ReadOnly,WorkspaceWrite,FullHost}` semantics, does **not**
   change `ApprovalMode`, does **not** enable network isolation by default, and
   does **not** alter macOS Seatbelt or Windows behavior.
6. The macOS Seatbelt read posture is explicitly out of scope. Its containment is
   write-confined by construction and its credential protection is a curated deny
   list; completing that list is a separate milestone and does not require a
   policy decision.
7. If measurement shows that a narrowed grant cannot be made to work for a
   toolchain the repository supports, this ADR is superseded rather than
   silently amended, and the residue is recorded as a named limit.

## Consequences

### Positive

The default contained command can no longer read the daemon's environment through
`/proc`, and the abstract-socket and signal channels are closed at no
namespace cost. The enforcement descriptor stops implying a socket boundary it
does not have. Linux keeps a stricter read posture than macOS, which is the
existing design intent rather than a new asymmetry.

### Negative

`/proc` and `/run` narrowing carries a real compatibility risk and will need a
toolchain matrix that the repository does not have today. Some hosts may end up
with a recorded residue rather than a clean removal.

### Neutral or deferred

The protected mount view remains the target end state and the only tier that
hides pathname sockets. Promoting it stays blocked on the
`linux-execution-isolation-roadmap.md` production admission, which this ADR does
not touch. Network isolation remains opt-in and unrelated to this decision.

## Compatibility and migration

No persisted configuration, durable payload, protocol frame or public DTO
changes. `SandboxLaunchSpec` and the helper status protocol are unchanged; the
obtained-scope fact is an in-memory `BackendEnforcement` value that is not
serialized to durable storage. A host that cannot obtain ABI 6 continues to run
the filesystem-only tier and reports the scope as absent rather than claiming it,
which is the existing `PartiallyEnforced` convention. No migration is required
and no legacy path is removed.

## Security and reliability implications

The change is restriction-only at the kernel boundary: `Scope` cannot widen a
ruleset, and removing a path from `read_paths` cannot grant one. The reliability
risk is the inverse of the security gain — a narrowed grant fails a command that
previously worked, and that failure must surface as an ordinary command failure,
never as a silent widening. The milestone therefore forbids any fallback that
re-adds a broad grant on failure, and requires the toolchain matrix to be
committed so the residue is reproducible rather than discovered in production.

## Verification

- A native Linux fixture asserts that `Scope::SIGNAL` and
  `Scope::ABSTRACT_UNIX_SOCKET` are in effect on a `LegacyPrivateHelper` launch:
  a contained child cannot signal a same-uid process outside the scope, and
  cannot reach an abstract socket bound outside it.
- A native Linux fixture asserts `/proc/<pid>/environ` for a same-uid process
  outside the target is **not** readable under the default policy, with a
  positive control proving the probe can read it when the grant is restored.
- A committed toolchain matrix (shell, Git, Cargo, Python, Node) passes with the
  narrowed grant set, and each removed path is named with the measurement that
  justified its removal.
- The enforcement descriptor reports the obtained scope ABI and the socket
  posture; a test fails if the descriptor claims socket isolation the mechanism
  did not obtain.
- No test may pass by skipping: an unavailable Landlock host fails the fixture
  rather than reporting green.

## Supersession

None. This ADR becomes `accepted` or is rejected as a whole; partial acceptance
is recorded as a named limit in the milestone rather than as an amendment here.
Reassess if the protected mount view is promoted to the default, at which point
clauses 1-2 are subsumed by ADR-0046's namespace tier.
