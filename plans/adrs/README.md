# Architecture Decision Records

This directory contains durable decisions that affect CodeGG architecture across milestones or subsystems.

Use an ADR when a question cannot be answered safely inside one implementation plan without establishing a reusable architectural contract.

## Naming

```text
ADR-NNNN-short-title.md
```

Numbers are monotonically increasing and never reused.

## Cross-branch identity migration

The 2026-10-13 Windows/TUI branch collision was resolved before PR #111
integration. Mainline TUI ADR-0021 and ADR-0022 retain their original identities.
The Windows decision records now use ADR-0023 through ADR-0026; the exact former
records remain under `plans/archive/windows-adr-id-aliases/`, with the source
SHA, content fingerprints, and disposition recorded in
`plans/adrs/windows-adr-id-migration.md`. The aliases are historical provenance,
not active ADR allocations.

## Linux/command identity migrations (2026-10-14 and 2026-10-15)

C004 archived the exact pre-migration Linux and command ADR records under
`plans/archive/linux-command-adr-id-aliases/` and recorded source Git blob IDs,
SHA-256 digests, and the identity mapping in
`plans/archive/linux-command-adr-id-aliases/migration.json`. Linux execution
isolation was assigned ADR-0031 (accepted 2026-10-14); command syntax authority was
ADR-0032 (accepted, unchanged). On reconciliation with current main, ADR-0031 was
already the accepted authenticated idle-continuation decision. The Linux decision
therefore retains its substance and accepted status under canonical ADR-0033;
idle continuation keeps ADR-0031, and command syntax then kept ADR-0032. A later
mainline acceptance assigned ADR-0032 to agent-workstream coordination, so command
syntax now keeps its substance and accepted status as ADR-0034. No decision's
substance or disposition changed. ADR-0033 acceptance selects the Linux
architecture but does not register or enable a production backend; native
qualification and rollout gates remain in the Linux roadmap. The concurrent
Windows identity migration reserves ADR-0029/ADR-0030; Windows ADR-0027 and
ADR-0028 retain their separate meanings. A number-only reference must be
interpreted by subsystem and path. The 2026-10-15 rename preserves the Linux ADR's
decision and accepted date while resolving the cross-branch identity collision.

`plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`
records the migration contract and
`plans/closure/linux-execution-isolation/012-status.md` records its evidence.

## Status lifecycle

```text
proposed -> accepted -> deprecated or superseded
         `-> rejected
```

Accepted ADRs are historical records. Do not rewrite an accepted ADR to make a later decision appear original. Create a new ADR and mark the old one superseded.

## ADR template

```markdown
# ADR-NNNN: Title

Status: proposed

Date: YYYY-MM-DD

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#...`
- `plans/001-terminology-and-domain-model.md#...`

Affected subsystem roadmaps:

- `plans/subsystems/...`

## Context

Describe the architectural problem, existing implementation, constraints, and why the decision is required now.

## Decision drivers

- ...

## Considered options

### Option A — Name

Description, benefits, costs, and failure modes.

### Option B — Name

Description, benefits, costs, and failure modes.

## Decision

State the selected option precisely, including ownership and interface boundaries.

## Consequences

### Positive

- ...

### Negative

- ...

### Neutral or deferred

- ...

## Compatibility and migration

Describe storage, protocol, configuration, API, and operational migration requirements.

## Security and reliability implications

Describe authorization, secret handling, contention, cancellation, restart, recovery, and denial-of-service effects.

## Verification

Describe the evidence required to prove implementations conform to this decision.

## Supersession

None.
```

## ADR threshold

An ADR is normally required when a decision:

- changes a canonical identity or ownership boundary;
- introduces a new daemon-to-daemon, frontend, provider, or storage protocol;
- selects a durable external standard or dependency;
- changes authentication or authorization semantics;
- changes scheduler or execution authority;
- changes consistency, replication, recovery, or fencing semantics;
- establishes a public compatibility contract;
- materially changes a long-term non-goal.

An ADR is usually unnecessary for local refactors, internal naming cleanup, implementation-specific data structures, or reversible optimizations that preserve established contracts.

## Cross-branch numbering note (2026-10-13)

The Windows sandbox workstream `work/windows-native-sandbox-foundation` already allocates ADR-0019 (staged workspace projection) and ADR-0020 (successor sandbox feasibility), even where an older `main` snapshot lists only ADR-0018. New editor-completion planning therefore reserves **ADR-0021**, currently **proposed**, to avoid cross-branch number reuse. Its presence is not acceptance or implementation authorization.

## Cross-branch ADR-0022 reservation (2026-10-13)

The post-closure TUI remote-file-tree authority workstream reserves `ADR-0022-daemon-authorized-workspace-directory-listing.md` as **proposed**. This supplements accepted ADR-0021 and does not constitute acceptance. Verify across active branches before allocating the next ADR number.

## Windows successor decisions after PR #111 (2026-10-13)

Renumbered 2026-10-14: the Windows successor decisions first landed as
ADR-0027/ADR-0028 on their planning branch. The Linux/command identity
migration assigned those numbers to ADR-0031/ADR-0032 before branch
reconciliation. The Windows records are ADR-0029/ADR-0030 below; the old
numbers were never active as Windows ADRs and must not be reused for Windows
references. Linux/command historical aliases and fingerprints are recorded in
`plans/archive/linux-command-adr-id-aliases/`.

- ADR-0029 — accepted for **bounded research, test-only successor adapters, and internal capability contract** only. Neither the MXC nor dedicated-principal backend is selected for production; C029/C034/C038 no-admission remains authoritative.
- ADR-0030 — **proposed**, not accepted: an expressly named broad-read, workspace-write compatibility profile would require independent maintainer authorization and migration/approval review. Existing ReadOnly/WorkspaceWrite contracts are not reinterpreted.
- ADR-0031 — **accepted** authenticated idle continuation and delegated turn admission (current-main decision).
- ADR-0032 — **accepted** daemon-owned agent workstream coordination and explicit shared-checkout mutation; production shared writes remain gated on M005.
- ADR-0033 — **accepted** composable Linux execution isolation; production backend registration and rollout remain gated by native qualification.
- ADR-0034 — **accepted** command syntax is advisory and execution authority is enforced; both identity migrations preserved this decision's substance and accepted status.
- New handoffs C041–C048 are registered in the Windows subsystem roadmap and active planning registry. Do not mark them implemented from the presence of documents.

## ADR-0032 workstream coordination (2026-10-15)

`ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md` was
accepted on 2026-10-15 and merged to main in commit `171ef93b`. It does not
modify accepted ADR-0002, admit shared-write child execution, or confer new
capability by existing group membership. M005 is active for writer-containment
design and qualification; shared-write execution remains unavailable pending
real tool-path confinement and lifecycle recovery evidence.

## ADR-0034 command-authority identity correction (2026-10-16)

Current main assigned ADR-0032 to the accepted agent-workstream decision. The
accepted command-syntax decision therefore retains its substance and status as
ADR-0034. ADR-0031 remains authenticated idle continuation and ADR-0033 remains
composable Linux execution isolation. No active identifiers collide.
