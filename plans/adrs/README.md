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

## Linux/command identity migrations (2026-10-14 onward)

The C004 identity corrective archived the exact pre-migration Linux and command
records under `plans/archive/linux-command-adr-id-aliases/`, with source blob IDs,
content digests, and mappings in `migration.json`. Reconciliation with current
main preserves all main ADR allocations through ADR-0043. The accepted command
authority decision is retained under ADR-0045 and the accepted Linux isolation
decision under ADR-0046. Their substance and acceptance status are unchanged;
production Linux backend registration and rollout remain gated by native
qualification. The proposed contained-Git live-object decision is retained as
ADR-0044 and does not authorize replacing the immutable trusted-Git route.

A second collision was reconciled on 2026-10-21: main had allocated ADR-0043 to
the accepted session-owned subagent lifecycle projection decision and ADR-0042 to
the accepted composer normal-mode key ownership decision, so the Linux isolation
record moved from ADR-0043 to ADR-0046 and the command authority record from
ADR-0042 to ADR-0045. Main wins both numbers; the decision content, status, and
acceptance dates are unchanged.

`plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`
records the original migration contract and
`plans/closure/linux-execution-isolation/012-status.md` records its historical
evidence. The current canonical mapping is recorded in
`plans/adrs/linux-command-adr-id-migration.md`.
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
- ADR-0031 — **accepted** authenticated idle continuation and delegated turn admission (current-main decision).
- ADR-0032 — **accepted** daemon-owned agent workstream coordination and explicit shared-checkout mutation; production shared writes remain gated on M005.
- ADR-0033 — **accepted** directory-tied project identity and client working directory.
- ADR-0034 — **accepted** editor syntax-tree dependency (current-main decision; Option B accepted 2026-10-28). See `ADR-0034-editor-syntax-tree-dependency.md`.
- ADR-0035 through ADR-0041 — current-main accepted decisions; see their individual records.
- ADR-0042 — **accepted** composer normal-mode key ownership (current-main decision).
- ADR-0043 — **accepted** session-owned subagent lifecycle projection (current-main decision; Option A accepted 2026-10-19).
- ADR-0044 — **proposed only** contained Git execution with live mutable ODB reads and brokered metadata promotion; test-only proof is permitted, but replacing the immutable trusted-Git route requires independent maintainer acceptance and native security/performance admission. See `ADR-0044-contained-git-live-objects-and-metadata-authority.md`.
- ADR-0045 — **accepted** command syntax is advisory and execution authority is enforced.
- ADR-0046 — **accepted** composable Linux execution isolation; production backend registration and rollout remain gated by native qualification.
- ADR-0047 — **accepted** restricted CodeGG-managed shared-checkout writer contract (option B, accepted 2026-10-19); it enables no shared mutation by itself. Renumbered twice as `main` took the lower numbers: ADR-0042 and then ADR-0044. Neither number may be reused for this decision. See `ADR-0047-restricted-agent-shared-checkout-writer-contract.md`.
- ADR-0048 — **accepted** contained Git execution for managed-worktree creation (accepted 2026-10-22); it authorizes exactly one contained Git operation shape. See `ADR-0048-contained-git-execution-for-managed-worktree-creation.md`.
- ADR-0049 — **accepted** open, provider-scoped reasoning control vocabulary with a faithful wire intent and a daemon-owned selection (option B, accepted 2026-10-21). Renumbered twice as `main` took ADR-0044 for the contained-Git live-object decision and then ADR-0048 for the contained-Git managed-worktree creation decision; neither number may be reused for this decision. See `ADR-0049-open-provider-scoped-reasoning-control-vocabulary.md`.
- ADR-0050 — **accepted** per-model reasoning capability authority and local admission policy (option B, accepted 2026-10-21): layered authority, narrowing-only overlays, fail-closed on an undeclared dimension. Renumbered twice: `main` took ADR-0045 for the command syntax and execution authority decision (2026-10-22), and then this line's own open-vocabulary record vacated ADR-0049 when `main` published its own ADR-0048 (2026-10-23). Neither number may be reused for this decision. See `ADR-0050-per-model-reasoning-capability-authority.md`.
- ADR-0051 — **accepted** (option B, accepted 2026-10-24): the durable delegated-run transcript and the continuation authority for a delegated run. Option B — a dedicated store keyed by `AgentRunId`, redacted **at write**, bounded per row and per run — is the accepted decision; option A (status quo) and option C (a real `session` row per delegated run) are rejected. It does **not** amend ADR-0031 (the durable notification and idle-continuation machinery), ADR-0041 (the human steers the parent, not the child) or ADR-0043 (the session-owned delegated lifecycle projection); it adds a transcript artifact to a delegated run and names what may cross the frontend seam. Acceptance grants the **authority**, not an implementation: C002, C003 and C004 each still carry their own closure gate. See `ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`.
- ADR-0052 — **accepted** cross-frontend command execution ownership (option B, accepted 2026-10-24): the daemon owns the command catalog and executes daemon-effect commands under `command.execute`, each frontend dispatches frontend-effect commands from a bounded declared set, command identity is shared while display names may differ, and a daemon-effect command is audited as a canonical `Run`. Its acceptance carried the canonical amendments the ADR made contingent on it — the cross-frontend `Command` definition in `plans/001-terminology-and-domain-model.md`, the desktop-frontend clause in `plans/000-long-term-specification.md` §4.4, the command-catalog clause in §24, invariant 24, and the desktop consumer named in Phase 5 of `plans/002-long-term-roadmap.md`. See `ADR-0052-cross-frontend-command-execution-ownership.md`.
- ADR-0055 — **accepted** Option A (user acceptance, 2026-10-27): persistent session-relative working directory stays inside the immutable session workspace root; cross-root changes use authorized session launch/fork/switch flows. W004 remains blocked until W002 strictly closes. See `ADR-0055-session-relative-working-directory-authority.md`.
- ADR-0056 — **accepted Option B (2026-10-28)**: typed daemon-owned LSP operations and low-frequency command dispatch, narrow `lsp.control` authorization, ephemeral session-bound previews, and standalone TUI compatibility. See `ADR-0056-daemon-lsp-command-and-runtime-control-surface.md`.
- New handoffs C041–C048 are registered in the Windows subsystem roadmap and active planning registry. Do not mark them implemented from the presence of documents.
## ADR-0032 workstream coordination reservation (2026-10-15)

ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md was
accepted on 2026-10-15 and merged to main in commit `171ef93b`. It does not
modify accepted ADR-0002, admit shared-write child execution, or confer new
capability by existing group membership. M005 is active for writer-containment
design and qualification; shared-write execution remains unavailable pending
real tool-path confinement and lifecycle recovery evidence.

## ADR-0047 restricted shared-checkout boundary reservation (2026-10-19)

The M005 writer-containment corrective proposed
ADR-0047-restricted-agent-shared-checkout-writer-contract.md and recorded a
narrower CodeGG-mediated checkout writer-coordination boundary (external
same-user processes are not contained). Filing the ADR neither authorized new
checkout sharing nor amended accepted ADR-0002 or ADR-0032. Implementation
corrective C001 established the contract and decision evidence: the 48-root
writer inventory, the disposition manifest `docs/writer-disposition.toml` and
the static guard `scripts/check_writer_disposition.py`.

**Update — accepted 2026-10-19.** The maintainers accepted **option B**
(the restricted CodeGG-managed coordination profile) after reviewing that C001
package. The acceptance is recorded in the ADR's own "Acceptance record"
section. It unblocks **C002 only** — the durable, generation-fenced checkout
writer permit and its lifecycle service. C003–C006 remain blocked on their own
predecessor closures. Acceptance does **not** enable shared-checkout mutation:
no permit exists, no writer is fenced, and no user-visible shared-mutation
selector is activated. Option A (universal filesystem exclusion) remains
deferred to a separate OS-isolation project; option C (advisory path claims)
remains rejected. Accepted ADR-0002 and ADR-0032 are unmodified and the legacy
isolated behavior is unchanged.
Check concurrent branch ADR identity allocation before integration.
