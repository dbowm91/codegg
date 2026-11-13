# Sandbox Containment Audit Corrective Roadmap

Status: conditionally closed — C001 and C005 closed; C002–C004 and C006 carry named outstanding evidence in their closure records

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8` (2026-11-10).

Predecessors: `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md`
(conditionally closed, C001-C003 closed), `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
(conditionally closed), `plans/subsystems/linux-execution-isolation-roadmap.md` (active;
production backend unqualified), and `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`
(active planning; C005 closed). Historical closure and evidence are referenced, never
overwritten: this roadmap adds findings from a new audit at a new baseline and does not
reopen any closed milestone.

Long-term references:

- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Related ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md` (accepted)
- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md` (accepted)
- `plans/adrs/ADR-0046-composable-linux-execution-isolation.md` (accepted)
- `plans/adrs/ADR-0061-default-linux-containment-policy.md` (accepted 2026-11-10 — the C002 gate is discharged)

## 1. Purpose and ownership boundary

Close the gaps found by a platform audit of the shipped containment backends on
Linux and macOS. The audit's method and its evidence are recorded in each
milestone's §3; the summary is in §4 below.

This roadmap owns:

- the backend-neutral **allowance and deny tables** (`policy.rs`) as they apply to
  the two shipped backends;
- the **macOS Seatbelt profile contract** and its credential deny list;
- the **default Linux containment policy** — which Landlock rights and scopes a
  contained launch obtains when no opt-in tier is requested — subject to ADR-0061;
- the **truthfulness of the enforcement descriptor** that an operator reads;
- the **containment test suite and its guards**, as evidence that the above hold.

This roadmap does **not** own: the Bubblewrap mount-view or namespace adapter, the
Linux production admission decision, the Windows backend, provider credentials,
approval routing, or any network-isolation capability. Those remain with their
existing owners (`linux-execution-isolation-roadmap.md`,
`windows-native-sandbox-roadmap.md`, ADR-0004).

The containment design itself is not reopened: `src/security/sandbox/`'s
policy/backend split, the one-shot `codegg-sandbox-helper` trust channel, and
`ManagedProcessService` as sole child owner all stay canonical. This roadmap
changes **what policy is compiled**, not **who compiles it**.

## 2. Work classification

### Invariants

- A constrained request never silently runs with host authority, and an
  unavailable backend refuses rather than degrades. (Holds today; must keep
  holding.)
- A declared credential exclusion actually resists the read and the write on
  every shipped backend. (Partially holds: the macOS list is incomplete — C001.)
- A contained Linux child cannot read a same-uid process's environment or reach a
  host service socket through a broad runtime-root grant, unless the enforcement
  descriptor says so. (Does not hold today — C002, C003.)
- An enforcement descriptor reports what the mechanism **obtained**, never what
  the profile **requested**, and never a guarantee with no producer. (Partially
  holds — C003.)
- The containment suite fails rather than skips when a qualified host is
  expected, and its denials are proved non-vacuous. (Does not hold today — C004.)
- Every guard that protects a silent-defect class is wired into both `quick` and
  CI, or is recorded as deliberately unwired with the reason. (Does not hold
  today — C005.)
- Descriptor hygiene and grant-path identity pinning apply to **every** contained
  launch, not only the opt-in tier. (Does not hold today — C006.)

### Capabilities

- An operator can tell, from the descriptor alone, which of filesystem read,
  filesystem write, process environment, host sockets, and network a given launch
  actually confined.
- `WorkspaceWrite` on macOS keeps the workspace writable while the documented
  credential set is unreadable, without breaking the git/Cargo/Python/Node
  toolchain.

### Infrastructure

- Completed credential deny list with a per-path justification record.
- A committed toolchain matrix that justifies each removed Linux read root.
- Kernel-asserted denial fixtures with positive controls and defect-injection
  non-vacuity controls.
- Wired guards with `--self-test`.

### Polish

- Doc corrections where a comment describes behaviour the code does not have.
- Removal or explicit disposition of the dead guarantee-negotiation surface.

## 3. Non-goals

- **No new sandbox framework.** No container runtime, VM, gVisor, mandatory
  Bubblewrap, setuid helper, external DNS proxy, or host firewall mutation.
- **No network isolation by default.** `--unshare-net` stays opt-in; this roadmap
  never claims egress confinement the mechanism did not obtain.
- **No promotion of the Linux production backend.** `linux-execution-isolation`
  owns that admission and its decision is unchanged.
- **No full macOS read allowlist.** The audit confirmed the read-broad posture is
  deliberate, tested, and pinned (`sandbox_containment.inc` asserts
  `READ-OUTSIDE-LEAK` for Seatbelt). This roadmap completes the **deny** list and
  does not convert macOS to deny-first; that would be a tool-compatibility
  redesign and belongs to its own ADR.
- **No change to `SandboxProfile`, `ApprovalMode`, or approval routing.** Profile
  semantics are unchanged; `FullHost` stays explicit-only.
- **No Windows work.** The Windows backend keeps its own roadmap and remains
  unavailable.
- **No `App`/TUI surface, no provider credential handling, no storage migration.**
- **No reopening of closed milestones.** Where a closed record's claim is
  narrowed by a new finding, the finding is recorded here and the old record is
  referenced, not rewritten.

## 4. Current state

The audit measured the shipped backends at the baseline above. The full evidence
is in each milestone; the load-bearing facts are:

**One profile name, three postures.** `ReadOnly` and `WorkspaceWrite` map to
`SandboxConfig` with no Linux opt-in, so a default contained Linux launch is
`LinuxLaunchPlan::LegacyPrivateHelper` — Landlock filesystem rules only, with no
PID/IPC/network namespace, no seccomp, no descriptor hygiene, and no Landlock ABI
6 `Scope`. macOS receives a single tier: `(allow default)`, then
`(deny file-write*)`, then per-root write re-allows, then read and write denies
for the curated deny list.

**macOS read confinement is the deny list.** `sbpl_profile` never emits
`read_roots`; `BackendPolicy::read_roots` is computed, validated, and discarded on
that backend. The deny list therefore *is* the read protection, and it omits
`~/.git-credentials`, `~/.config/git/credentials`, `~/.m2/settings.xml`,
`~/.gradle/gradle.properties`, `~/.yarnrc.yml`, `~/.password-store`,
`~/.local/share/keyrings`, `~/.config/op`, `~/.vault-token`, `~/.pgpass`,
`~/.my.cnf`, the shell history files, and the macOS private stores
(`Library/Messages`, `Library/Mail`, `Library/Safari`, `Library/Cookies`).
`~/.git-credentials` is the plaintext store written by `credential.helper=store`,
and git is the tool the agent runs constantly.

**The default Linux grant set reaches the same secrets by another route.**
`tool_read_allowances()` grants `/proc`, `/sys`, `/var/run`, `/etc`, `/opt`.
`/proc` plus a shared uid makes `/proc/<daemon-pid>/environ` readable, which is
the material the credential deny list protects by path. `/var/run` plus the fact
that Landlock does not mediate `connect(2)` on a pathname AF_UNIX socket leaves
session-bus, agent and container-runtime sockets reachable whenever the socket
mode admits the target uid; the ABI 6 scope covers abstract sockets only.

**Enforcement claims are partly unbacked.** Two surfaces look similar and are not.
`host_admission.rs` has no production caller but **is** live test infrastructure:
the `host_admission_manifest` family member (`tests/security_permission/
host_admission_manifest.inc`, declared in the family `mod.rs`) consumes it, and its
closure record records the intent. `guarantee.rs` is genuinely orphaned: nothing
outside its own `#[cfg(test)]` module and the `sandbox.rs` re-export names
`negotiate()`, `verify_helper_backend()` or `AttestedCapabilities`. The finding
that matters is narrower than "dead code": the parent accepts
`SandboxLaunchOutcome::Enforced { backend }` without checking that the backend is
the one it selected, although `verify_helper_backend()` exists for exactly that.
`SandboxLaunchOutcome::BubblewrapReady` has no producer
but is accepted by the status decoder as a valid setup frame. `seatbelt::probe()`
checks only that `/usr/bin/sandbox-exec` exists, not that `sandbox_init` works, so
capability can report `Available` and then fail every launch (fail-closed, but the
report overstates). `apply_process_scopes()` returns the effective ABI and the
helper discards it, so the strongest guarantee list is inferred from the spec
rather than observed.

**Evidence integrity is weaker than it reads.** All 21 macOS-reachable containment
tests can pass green through `require_backend!` on a host whose probe fails. The
rename/hardlink escape test is gated `cfg(target_os = "linux")` although the
profile that blocks it on macOS is the same one under test. The curated deny list
is never asserted against a real kernel: every denial test injects its own
`deny_paths`. The only defect-injection non-vacuity control in the whole suite is
in the Linux scratch fixture.

**Two sandbox guards do not protect anything automatically.**
`scripts/check_sandbox_policy_wiring.py` (580 lines) is invoked by no script and
no workflow; it currently exits 1 on the tree, but on a **stale pattern** — it
looks for `sandbox_config_for_profile` in `executors.rs` while the code calls the
newer `sandbox_config_for_durable_payload`, so the failure is a false positive and
the property it intends to check does hold. `scripts/check_sandbox_contract.py` is
wired into `quick` and CI but its `--self-test` is invoked by neither. The hosted
macOS lane's path filter names `tests/sandbox_containment.rs`, which does not
exist.

**Two hardening measures stop at the opt-in boundary.**
`close_unapproved_fds` is true only when `linux_process_isolation` is set, so
macOS and the default Linux tier pass the daemon's inherited descriptor table to
the target. The Landlock route re-opens each grant with `PathFd::new(path)` after
`BackendPolicy::normalized` canonicalised it, so it carries the check-then-open
window that the mount view closes with `openat2` plus a `dev`/`ino` identity
check.

## 5. Target architecture

One backend-neutral policy compiles to two mechanisms, and the descriptor states
the mechanism's obtained facts per dimension. The end state this roadmap reaches:

| Dimension | macOS Seatbelt | Linux default | Linux mount view (opt-in, unchanged) |
|---|---|---|---|
| Filesystem write | confined to write roots | confined to write roots | confined to leased mounts |
| Filesystem read | broad, minus a **complete** curated deny list | curated allowlist, minus broad runtime roots | read-allowlisted mount graph |
| Process environment | not exposed by path | `/proc/<pid>/environ` not reachable | private procfs |
| Host sockets | reachable (disclosed) | abstract scoped; pathname reachable and **disclosed** | hidden by mount view |
| Network | unrestricted (disclosed) | unrestricted (disclosed) | `--unshare-net` when requested |
| Descriptor | obtained facts only | obtained facts incl. scope ABI | obtained facts incl. netns receipt |

Nothing in this table is a new mechanism. Every row is either an existing
mechanism applied to a path that already had it (C002, C006), an existing table
completed (C001), an existing descriptor made truthful (C003), or an existing
fixture made non-vacuous (C004, C005).

## 6. Dependency graph

```text
ADR-0061 (accepted 2026-11-10 — gate discharged)
                                  │
                                  v
C001 macOS deny set ── operational ──> C002 Linux default policy
     (invariant)                          (invariant)
       │                                     │
       │ operational (shared fixture)        │ operational (shared limits text)
       v                                     v
C004 evidence integrity              C006 descriptor hygiene
     (invariant)                         + grant-path pinning
       │                                     (invariant)
       │ interface (strict-mode marker)        │
       v                                        │ operational
C005 guard and lane integrity                   v
     (infrastructure)                     C003 claim truthfulness
                                            (infrastructure)
```

**Satisfied:** C002 ← ADR-0061 acceptance. The default Linux containment policy
was reserved to an explicit decision by ADR-0046 §8; ADR-0061 Option B was
accepted on 2026-11-10, so the edge is closed and C002 was handed off. C002 is
now implemented and `conditionally closed`: the outstanding conditions are Linux
kernel evidence, not the ADR, and they are named in
`plans/closure/sandbox-containment-audit-corrective/002-status.md`.

**Interface:** C005 ← C004. C004 defines the strict-mode marker that makes the
containment suite fail rather than skip on a qualification lane; C005 sets it in
the workflow. The name is C004's to define and C005's to consume.

**Operational:** C001 before C002 (both edit the `policy.rs` allowance and deny
tables); C004 after C001 (both extend `sandbox_containment.inc`); C006 after C002
(both rewrite the Landlock limits disclosure); C003 after C006 (both edit the
enforcement-descriptor surface). Operational edges are sequencing, not semantic
dependencies: any of these plans is independently executable, but two that share a
file must not run concurrently.

**Soft:** none. No milestone in this roadmap needs another's output to be correct.

## 7. Milestones

### Milestone C001 — macOS credential deny-set completion

Class: invariant

Objective: make the macOS credential exclusion boundary cover the material an
agentic coding tool can actually reach, with a per-path justification and a
kernel-asserted denial for each addition.

Dependencies: none hard. Operational predecessor for C002.

Deliverable boundary: `policy::sensitive_deny_paths()` for unix plus a new
kernel-asserted fixture. No change to `sbpl_profile`'s rule order, no change to
the read-broad posture, no change to the Linux read allowlist.

User or operator value: a prompt-injected `cat ~/.git-credentials` stops working
on macOS, which is the platform where the deny list is the only read protection.

Exit conditions: every added path has a recorded classification (credential-only,
config-with-embedded-secret, or private user store) and a recorded breakage
assessment; the fixture proves a real kernel denial per path with a positive
control; the accepted breakage is documented in `architecture/security.md`.

Deferred work: converting macOS to a deny-first read allowlist. That is a
tool-compatibility redesign and needs its own ADR.

### Milestone C002 — Linux default-tier containment policy

Class: invariant

Objective: apply Landlock ABI 6 scopes on the default route and narrow the default
read-grant set so `/proc` and `/var/run` no longer expose same-uid process
environment or host service sockets.

Dependencies: none outstanding — `ADR-0061` Option B was accepted 2026-11-10.
Operational after C001.

Deliverable boundary: `policy::tool_read_allowances()`, `landlock::apply`,
`landlock::apply_process_scopes`, the helper's Landlock dispatch, and a committed
toolchain matrix. No namespace requirement, no Bubblewrap change, no network
change.

User or operator value: a contained command on Linux can no longer read the
daemon's environment through `/proc`, and the abstract-socket and signal channels
close at no namespace cost.

Exit conditions: the scope fixture proves signal and abstract-socket scoping on a
`LegacyPrivateHelper` launch; the `/proc` fixture proves a same-uid environment is
unreadable with a positive control; the toolchain matrix passes and each removed
root names its measurement; the descriptor reports the obtained scope.

Deferred work: pathname AF_UNIX socket isolation. Only the mount view hides those,
and its production admission is owned elsewhere.

### Milestone C003 — Enforcement claims match the mechanism

Class: infrastructure

Objective: remove or wire every claim surface that has no producer, and make the
descriptor report observed facts where it currently infers them.

Dependencies: none hard. Operational after C006.

Deliverable boundary: `guarantee.rs`/`host_admission.rs` disposition, the
`BubblewrapReady` decoder arm, `seatbelt::probe()` depth, the discarded scope ABI,
and one doc correction. No behaviour change to any launch that succeeds today.

User or operator value: "contained" stops being a claim an operator has to
discount.

Exit conditions: each item is either wired with a test or deleted with a recorded
reason; no enum variant is accepted by the decoder without a producer; the probe
either verifies the mechanism or its limitation is documented; the effective scope
ABI is recorded rather than discarded.

Deferred work: full requested-vs-obtained negotiation at launch. That changes
refusal semantics and would need its own decision.

### Milestone C004 — Containment evidence integrity

Class: invariant

Objective: make the containment suite fail rather than skip when a qualified host
is expected, and prove its denials are not vacuous.

Dependencies: none hard. Operational after C001. Interface provider for C005.

Deliverable boundary: `tests/security_permission/*.inc` and the strict-mode marker
contract. No production change.

User or operator value: a green containment suite means containment was observed,
on both platforms.

Exit conditions: the rename/link escape test runs on macOS; a defect-injection
control proves a widened grant is observable on macOS as it already is on Linux; a
strict-mode marker makes an unavailable backend fail; the `read_only` unit test
matches the production profile shape.

Deferred work: none.

### Milestone C005 — Sandbox guard and qualification-lane integrity

Class: infrastructure

Objective: make the guards that protect silent containment defects actually run,
and make the macOS lane trigger on the file it is supposed to protect.

Dependencies: interface — consumes C004's strict-mode marker.

Deliverable boundary: `scripts/check_sandbox_policy_wiring.py`,
`scripts/check_sandbox_contract.py` invocation, `scripts/verify.sh`,
`.github/workflows/shared-checkout-macos-qualification.yml`. No production change.

User or operator value: the guard set can no longer be green while a containment
property regressed.

Exit conditions: the policy-wiring guard's stale pattern is corrected and it
passes; it carries `--self-test`; both self-tests are invoked by `quick` and CI;
the lane's path filters name files that exist and the lane sets the strict-mode
marker.

Deferred work: none.

### Milestone C006 — Descriptor hygiene and grant-path pinning

Class: invariant

Objective: apply descriptor closure and grant-path identity pinning to every
contained launch, not only the opt-in tier.

Dependencies: none hard. Operational after C002.

Deliverable boundary: `managed_process.rs` descriptor configuration, the helper's
Landlock grant opening, and the limits disclosure. No change to what the mount
view already does.

User or operator value: an inherited descriptor can no longer reach a host file a
policy excluded, and a grant root cannot be swapped between canonicalisation and
ruleset construction.

Exit conditions: descriptor closure applies on macOS and on the default Linux
tier; the Landlock grant path is opened with no-follow resolution and re-verified
by identity; the mount-view limits disclose the inherited-descriptor channel they
currently omit.

Deferred work: none.

## 8. Cross-cutting requirements

### Storage and migration

None. No schema, migration, `STORAGE_LAYOUT_VERSION` bump, or persisted
configuration key changes. `SandboxLaunchSpec` and the helper status protocol are
unchanged; every new descriptor fact is in-memory only.

### Protocol and compatibility

The helper status protocol stays versioned at `SANDBOX_STATUS_VERSION = 1` unless
C003 changes a frame, in which case the version is bumped and the parent accepts
both shapes for one release. No public DTO, durable payload, or client changes.

### Security and authorization

Every change is restriction-only at the kernel boundary. No milestone may widen a
grant, remove a deny, add a fallback that restores a broad grant on failure, or
convert a refusal into a degradation. `FullHost` stays explicit-only; no milestone
here changes approval routing or profile semantics.

### Concurrency, cancellation, and recovery

The containment path is per-launch and stateless. C002 and C006 must preserve
fail-closed behaviour on partial setup: a failed scope or a failed grant open
aborts before target execution and reports `SetupError`/`Unavailable`, never a
contained claim.

### Observability and audit

The enforcement descriptor is the operator surface. C002 and C003 add obtained
facts (scope ABI, socket posture, proc posture) and C006 adds the descriptor
channel to the limits list. No new log stream and no secret in any descriptor.

### Documentation and static guards

`architecture/security.md` carries the invariant list and the "Backends report
obtained facts, not claims" rule; each milestone that changes a guarantee updates
it. C005 owns guard wiring; every new guard carries `--self-test` per the
repository's automatic-guard doctrine.

### Performance and resource use

C002 adds one `restrict_self()` per launch and removes path grants, so ruleset
construction gets cheaper. C006 adds one `fcntl` loop per launch. Neither needs a
wall-clock claim and none may be justified by a benchmark that was not run.

## 9. Verification strategy

- **Kernel-observed, per platform.** Every containment claim is backed by a real
  contained child whose denial is observed, not by profile text. Profile-text
  assertions stay as a fast structural check and are never the acceptance
  evidence for a guarantee.
- **Positive controls.** Every denial probe carries a positive control proving the
  probe can observe the allowed case, so a broken probe cannot read as a denial.
- **Defect injection.** At least one fixture per platform widens a grant
  artificially and observes the difference, so the denials are proved non-vacuous.
- **No green-from-skip.** A qualification lane sets the strict-mode marker so an
  unavailable backend fails the fixture instead of reporting green.
- **Committed matrices.** The Linux toolchain matrix that justifies each removed
  root is committed, so the residue is reproducible rather than discovered in
  production.
- **Guard non-vacuity.** Each guard that is wired carries `--self-test` and the
  self-test is invoked by the lane that runs the guard.

## 10. Risks and decision points

1. **`/proc` narrowing breaks a toolchain (C002, highest risk).** Node, Java and
   some Rust build scripts read `/proc`. Mitigation: measure first, prefer a
   narrower grant, and stop rather than restore broadly. The plan's stop condition
   is explicit and the residue is recorded as a named limit.
2. **`/run` narrowing breaks DNS (C002).** Some distributions resolve
   `/etc/resolv.conf` through `/run/systemd/resolve/`. Mitigation: name the
   specific file rather than the tree, and include it in the matrix.
3. **Deny-list additions break ordinary tools (C001).** `~/.m2/settings.xml` and
   `~/.gradle/gradle.properties` are build configuration that embeds credentials;
   denying them breaks Maven and Gradle builds that need them, and `.npmrc` sets
   that precedent. Mitigation: classify each path and record the accepted
   breakage; do not add a path whose breakage has no recorded justification.
4. **A deny path with hardlink aliases refuses the launch (C001).** Existing
   behaviour: `validate_denied_paths` refuses when a denied regular file has
   `nlink > 1`. Adding paths widens the set of hosts that can hit it. This is
   fail-closed and stays; it must be recorded so it is not mistaken for a new
   defect.
5. **ADR-0061 is amended after acceptance (C002).** It was accepted on
   2026-11-10, so the rejection outcome recorded when this roadmap was authored
   did not occur and C002 was implemented. An amendment that narrows the accepted
   policy would leave the affected residue named in the descriptor and in the
   closure record rather than restored to a broad grant — the same stop
   condition, applied later.
6. **Wiring the policy-wiring guard surfaces other stale patterns (C005).** The
   guard has 580 lines of checks that have never run. Mitigation: the plan treats
   each failure as a decision — fix the guard, or record why the property changed
   — and forbids silencing a check to get green.
7. **The macOS lane is not triggered by these changes (C005).** Fixing the path
   filter is in scope; re-qualifying the lane is not, and if the lane is currently
   red it stays red with the reason recorded rather than being disabled.

## 11. Completion definition

The roadmap is complete when: every credential path in C001's classification is
either denied and kernel-verified or recorded as an accepted residue; the default
Linux launch reports an obtained scope ABI and no longer reaches `/proc` or a host
service socket through a broad grant, or the residue is named; no claim surface in
`guarantee.rs`, `host_admission.rs`, the status decoder or the probe is without a
producer or a recorded reason; the containment suite cannot pass by skipping on a
qualification lane and carries a non-vacuity control on both platforms; every
sandbox guard is wired with an invoked `--self-test` or recorded as deliberately
unwired; descriptor hygiene and grant-path pinning apply to every contained
launch; and `architecture/security.md` describes the posture each backend actually
delivers.

A milestone is complete only when its closure record exists and says so.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 macOS credential deny-set completion | closed | `plans/implementation/sandbox-containment-audit-corrective/001-macos-credential-deny-set-completion.md` | `plans/closure/sandbox-containment-audit-corrective/001-status.md` | none |
| C002 Linux default-tier containment policy | conditionally closed | `plans/implementation/sandbox-containment-audit-corrective/002-linux-default-tier-containment-policy.md` | `plans/closure/sandbox-containment-audit-corrective/002-status.md` | Linux kernel scope evidence, the committed toolchain matrix, and the `/proc` narrowing with a positive control remain outstanding |
| C003 enforcement claims match the mechanism | conditionally closed | `plans/implementation/sandbox-containment-audit-corrective/003-enforcement-claims-match-the-mechanism.md` | `plans/closure/sandbox-containment-audit-corrective/003-status.md` | AC4 met in the tree (helper reports the returned scope ABI pre-exec; descriptor names it; absence is a limit; docs at `architecture/security.md:268`). Remaining: runtime refusal, non-vacuity demos, the Linux producer/consumer run, and the `python_script` `None` ambiguity |
| C004 containment evidence integrity | conditionally closed | `plans/implementation/sandbox-containment-audit-corrective/004-containment-evidence-integrity.md` | `plans/closure/sandbox-containment-audit-corrective/004-status.md` | Linux control run, mutation demonstrations, and the unavailable-backend arm remain outstanding |
| C005 sandbox guard and qualification-lane integrity | closed | `plans/implementation/sandbox-containment-audit-corrective/005-sandbox-guard-and-qualification-lane-integrity.md` | `plans/closure/sandbox-containment-audit-corrective/005-status.md` | none |
| C006 descriptor hygiene and grant-path pinning | conditionally closed | `plans/implementation/sandbox-containment-audit-corrective/006-descriptor-hygiene-and-grant-path-pinning.md` | `plans/closure/sandbox-containment-audit-corrective/006-status.md` | Linux runtime evidence, mutation demonstrations, and two §10 bound questions remain outstanding |

**The Linux evidence and the `/proc` narrowing are owned elsewhere, and one of
them is not evidence debt.** The cross-line corrective
`plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`
(registered `conditionally closed` in `plans/registry.md`, closure
`plans/closure/five-line-closure-corrective/001-status.md`) carries them as
WP B and WP C: WP B
sets `CODEGG_SANDBOX_QUALIFICATION` in the Linux job so `require_backend!` fails
loudly instead of returning, asserts or pins Landlock ABI ≥ 6, and wires the
committed three-run toolchain matrix as a CI step; WP C narrows the `/proc` and
`/var/run` tree grants in `src/security/sandbox/policy.rs` and adds the
`/proc/<other-pid>/environ` denial fixture **with a positive control**. WP C is a
**live** security narrowing — `/proc/<other-pid>/environ` is readable today —
and recording it as an outstanding evidence item would understate it. Both are
unobservable until the tree is committed and pushed, because
`#[cfg(target_os = "linux")]` modules do not compile on darwin.
