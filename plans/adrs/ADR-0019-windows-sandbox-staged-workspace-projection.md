# ADR-0019: Windows Sandbox Existing-Data Isolation Strategy

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

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

## Context

C013's same-revision Windows Server 2025 evidence (`6028e03f`, workflow `38296752862`) found that a populated workspace file and a credential hardlink shared one NTFS file ID. The current root-level ACL lease changed that shared object, and restoration returned DACL bytes while leaving descendant security descriptor control bits changed. Grant-only access also failed the fixture's allowed workspace read. The bounded test-support preflight therefore rejects populated ACL roots before mutation and child creation; production constrained Windows execution remains unavailable.

Recursive per-object mutation would need complete identity and alias discovery, race-safe pinning, bounds, and exact recovery while arbitrary host writers may operate on the tree. This contract has not been shown safe. The current path-prefix model cannot express different rights for hardlinks to one file object.

## Decision drivers

- Never grant a child access to denied host data through direct paths, reparse points, hardlinks, streams, inherited handles, or equivalent namespaces.
- Preserve the workspace read/write contract without mutating arbitrary user data or leaving descriptor drift.
- Reject unsupported or ambiguous identity/alias states before child resume.
- Keep production Windows `SandboxRequest::Required` unavailable until the security matrix passes.
- Keep any future projection bounded, disposable, recoverable, and owned by `ManagedProcessService`.

## Considered options

### Option A — Continue root-level ACL leases on existing trees

Rejected by C013 evidence. Directory ACL propagation does not provide a complete object journal; a hardlink alias shares the affected security descriptor; observed restoration left descendant control-bit drift.

### Option B — Enumerate and lease every existing object

Potentially explicit, but requires complete hardlink and namespace alias coverage, race control against arbitrary writers, bounded traversal, handling protected DACLs and exact restoration of all control metadata. C013 did not find or qualify a mechanism that satisfies those requirements. This remains unavailable pending a separate implementation plan and evidence.

### Option C — Stage an isolated workspace projection or use an isolated identity

Candidate for evaluation. A projection could expose only explicitly copied or materialized workspace inputs inside a per-run root, then collect approved outputs through a bounded interface. Alternatively, a separately provisioned identity could have a host ACL policy that denies sensitive data without changing user-owned descriptors. Both introduce compatibility, freshness, executable/toolchain, symlink/hardlink, secret-injection, and recovery questions; neither is selected by this proposal.

### Option D — Keep existing-data constrained execution unavailable

Safest current behavior and the active disposition. Populated roots remain rejected in test support, and production constrained requests remain unavailable. This is not a user capability closure.

## Decision

No new enforcement mechanism is accepted by this proposed ADR. The recommendation is to investigate Option C in a separately reviewed, bounded implementation plan while retaining Option D as the runtime disposition. Maintainers must accept or reject this ADR before any projection, identity provisioning, or production admission work begins.

## Consequences

### Positive

- Prevents the known root-lease alias and restoration findings from being presented as contained.
- Creates a decision boundary for future work without authorizing broad ACL mutation or implicit provisioning.

### Negative

- Existing populated workspace roots cannot be used by constrained Windows launches, limiting capability.
- A projection may require explicit input/output mapping and may not support tools that expect arbitrary host workspace mutation.

### Neutral or deferred

- C011's broader privilege, IPC, descendant, restart, and realistic-workspace qualification remains independently required.
- Windows network isolation remains outside this decision.

## Compatibility and migration

No runtime, storage, protocol, or configuration change is authorized. Any accepted projection design must define snapshot freshness, symlink/reparse and hardlink handling, output reconciliation, interruption cleanup, storage bounds, and migration before implementation.

## Security and reliability implications

The current fail-closed behavior remains authoritative. A future projection must prove that sensitive host data is not included through aliases or metadata channels, that children cannot escape the projection through inherited handles or reparse points, and that partial output/recovery cannot alter paths outside the authorized workspace. An isolated identity requires explicit administrator-controlled provisioning and reversible ACL policy; it must not be silently created by a tool invocation.

## Verification

Any implementation requires exact-SHA native Windows evidence for actual running-child direct, reparse, hardlink, ADS and post-creation probes; allowed workspace operations; replacement races; failure, cancellation and restart recovery; and exact state cleanup. It must also retain Linux/macOS regression checks and pass the independent C011 review before M004 admission.

## Supersession

None.
