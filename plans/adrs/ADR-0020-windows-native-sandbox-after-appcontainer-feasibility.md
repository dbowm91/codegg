# ADR-0020: Windows Native Sandbox After AppContainer Feasibility

Status: proposed

Date: 2026-10-12

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Affected subsystem roadmaps:

- `plans/subsystems/windows-native-sandbox-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`
- `plans/adrs/ADR-0019-windows-sandbox-staged-workspace-projection.md`

## Context

ADR-0019 selected a bounded AppContainer staged-projection experiment, with C014 as a hard feasibility gate. Exact-SHA Windows Server 2025 run `38316065754` showed a verified test-only AppContainer child could read and write a disposable projection, but Git initialization failed because the child could not open `/dev/null`. Git is the minimum tool required by C014. The host-alias, daemon IPC, inherited-handle, Cargo and Python probes did not run, so this result neither establishes a viable minimal toolchain nor qualifies host-read denial.

C014 is closed negative, and C015–C018 remain blocked. The repository still rejects production Windows `SandboxRequest::Required`; no filesystem or network enforcement is claimed. A new decision is needed before authorizing account provisioning or another implementation line.

## Decision drivers

- Keep constrained Windows execution unavailable unless each requested guarantee is actually enforced and independently reported.
- Do not mutate existing workspace, profile, or daemon IPC ACLs to make a tool run.
- Compare compatibility, operational setup, administrative consent, credential lifecycle, rollback, security boundaries and supported-host coverage.
- Treat the C014 observation as evidence about the tested runner and grants, not a blanket AppContainer result.
- Require an explicit new native feasibility gate before any snapshot, launch, or promotion implementation proceeds.

## Considered options

### Option A — Provision a dedicated low-privilege Windows identity

Evaluate a separate identity with bounded projection access, explicit runtime permissions, lifecycle management and optional identity-scoped network controls. This may improve compatibility with developer tools while keeping the child distinct from the interactive user. Costs include administrator provisioning, account and credential lifecycle, ACL design and recovery, supported-host requirements, and a new adversarial qualification matrix. No provisioning is authorized by this proposal.

### Option B — Retain constrained Windows execution as unavailable

Keep current fail-closed behavior and continue to support only the existing FullHost contract. This avoids introducing an unqualified security principal or operational dependency. It leaves Windows constrained execution unsupported and may limit parity with macOS/Linux.

### Option C — Select another OS-managed isolation boundary

Assess a separately isolated VM or Windows Sandbox style boundary with explicit workspace import/export. This can offer a stronger machine boundary but adds virtualization requirements, startup and storage cost, host integration constraints, and a distinct output authorization path. It requires its own bounded feasibility plan and must not reuse the failed AppContainer result as proof.

## Decision

No implementation option is selected in this proposed ADR. Maintainers must choose an option or request a bounded comparison before new Windows sandbox work is marked ready. Until then, retain fail-closed unavailability for constrained Windows requests. If a new experiment is approved, it must independently prove the minimum useful tool subset and actual denied host access before any dependent snapshot or promotion work begins.

## Consequences

### Positive

- Makes the C014 negative result an explicit architecture gate instead of allowing C015 to proceed by momentum.
- Preserves current production security behavior while alternatives are reviewed.
- Records dedicated identity and VM isolation as candidates without approving account, ACL, firewall, or virtualization changes.

### Negative

- Windows constrained execution remains unavailable while the proposal is unresolved.
- A future alternative may require administrator consent, additional CI infrastructure, or a narrower supported tool set.

### Neutral or deferred

- C015–C018 remain blocked. Their plans are not authorized for implementation unless a successor decision and strict-positive feasibility gate reopen the line.
- C011/M004–M006 remain blocked independently; this proposal does not close any historical security finding.

## Compatibility and migration

No code, storage, protocol, configuration, account, ACL, firewall, or workspace migration is authorized by this proposal. Existing Windows FullHost behavior and Unix backends remain unchanged. Production Windows constrained requests continue to fail closed.

## Security and reliability implications

Do not infer host denial from the C014 run: its alias, IPC, and inherited-handle probes were not reached. Do not infer network isolation from an AppContainer token or a dedicated identity. Any accepted successor must qualify direct and aliased reads, writes, IPC, inherited handles, descendants, cancellation, restart/recovery, and the native OS build matrix, with no broad user-profile or credential grant.

## Verification

The proposal is based on closure `plans/closure/windows-native-sandbox/014-status.md` and exact-SHA workflow `38316065754`. No implementation verification is applicable until maintainers accept a decision and a new bounded implementation plan is registered.

## Supersession

None. ADR-0019 remains an accepted historical decision to run the bounded experiment; its negative C014 result stops that line and does not alter its record.
