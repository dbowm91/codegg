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

**Active identity collision warning (2026-10-13):** combined `main` currently contains
two active ADR-0027 records (Windows bounded successor and proposed Linux
composable execution) and two active ADR-0028 records (proposed Windows broad-read
and accepted command authority). The earlier assertion of uniqueness is false.
Do not disable the existing ADR integrity guard to make this pass.

`plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`
is ready and proposes an identity-only migration of Linux/command decisions to
unused ADR-0029/0030 after rechecking current reservations, with historical
archives and exact scoped reference updates. **This plan has not executed the
rename.** Existing decisions and statuses remain unchanged; no Linux backend,
Windows production sandbox, or broad-read profile is admitted by this note.

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

Renumbered 2026-10-14: the successor decisions first landed as ADR-0027/ADR-0028
on the planning branch, but main had meanwhile assigned those numbers to the
Linux execution-isolation (ADR-0027) and command-syntax authority (ADR-0028)
decisions. The Windows records are ADR-0029/ADR-0030 below; the old numbers
were never active on main and must not be reused for Windows references.

- ADR-0029 — accepted for **bounded research, test-only successor adapters, and internal capability contract** only. Neither the MXC nor dedicated-principal backend is selected for production; C029/C034/C038 no-admission remains authoritative.
- ADR-0030 — **proposed**, not accepted: an expressly named broad-read, workspace-write compatibility profile would require independent maintainer authorization and migration/approval review. Existing ReadOnly/WorkspaceWrite contracts are not reinterpreted.
- New handoffs C041–C048 are registered in the Windows subsystem roadmap and active planning registry. Do not mark them implemented from the presence of documents.

## ADR-0032 workstream coordination reservation (2026-10-15)

ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md was
accepted on 2026-10-15 and merged to main in commit `171ef93b`. It does not
modify accepted ADR-0002, admit shared-write child execution, or confer new
capability by existing group membership. M005 is active for writer-containment
design and qualification; shared-write execution remains unavailable pending
real tool-path confinement and lifecycle recovery evidence.
