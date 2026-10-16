# ADR-0033: CodeGG-Mediated Checkout Writer Boundary for Shared Placement

Status: accepted — 2026-10-16; user authorization recorded in the work session. No implementation or shared-write authorization.

Date: 2026-10-16

Decision owners: project maintainers

Related:

- ADR-0002 — Agent run worktree isolation and control
- ADR-0004 — Permission and sandbox authority
- ADR-0032 — Daemon-owned agent workstream coordination and explicit shared-checkout mutation
- `plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md`
- `plans/implementation/agent-workstream-blackboard/008-comparative-trajectory-qualification.md`

## Context

M005 requires a product boundary for the writer-containment design before a
permit service is considered. A daemon lock can coordinate CodeGG-owned
execution paths only when every effective writer is mediated and remains
under verified ownership through quiescence. It cannot stop an editor or
unrelated process running as the same operating-system user from changing the
checkout.

The source audit also finds CodeGG-owned write surfaces outside an agent's
ordinary file-edit tool, plus PTY and protocol-server lifetimes that cannot
currently be bounded by the duration of a single tool call. An all-writers
claim would therefore be false. ADR-0032 remains accepted for its architecture
direction, but its isolated-worktree default and M005 shared-write gate remain
in force.

## Decision drivers

- Do not describe a daemon permit as protection against writers outside CodeGG.
- Preserve ADR-0002 isolated worktrees as the default.
- Require one generation-fenced permit authority for every enabled CodeGG
  mutation path in any future shared-placement profile.
- Fail closed for effect owners whose writes or effective process lifetime
  cannot be contained and attested.
- Treat external edits as concurrent checkout drift and detect them at
  guarded transaction boundaries; do not claim they were serialized.

## Considered options

### A — Universal same-user writer exclusion

Require the system to serialize every process running as the same OS user,
including unrelated editors and shells. A daemon mutex, advisory file lock,
process group, or Windows Job Object cannot establish this property. This
option remains unavailable unless a separate OS-identity or filesystem
boundary is designed and qualified.

### B — Restricted CodeGG-mediated shared-placement profile

Define the guarantee over CodeGG-owned mutation routes. External same-user
processes are outside the serialization contract, and concurrent external
changes are surfaced as conflicts. Within CodeGG, every enabled writer must
acquire one typed `(WorktreeId, lease_generation)` permit or be denied before
mutation/spawn. PTY and persistent LSP/MCP/plugin services remain denied until
their effective lifetimes are supervised and reconciled. Finite processes
remain denied on any platform that cannot prove descendant quiescence.

This is the selected decision. It does not relax sandbox or approval
ceilings, default isolation, or any existing WorkOrder policy.

### C — Add a scheduler resource key and call it serialization

A scheduler key cannot cover inline AgentTurn edits, direct project-file
APIs, or long-lived processes. Rejected as an incomplete authority boundary.

## Decision

“Serialized shared placement” means
serialization of *CodeGG-mediated* writers only. It will not promise
exclusion of external same-user processes. Guarded CodeGG transactions must
detect stale file state and return a typed conflict when an external edit is
observed; conflict detection is not external-writer protection.

Before Q1 permit-state implementation begins, M005 must enumerate every
CodeGG-owned writer reachable from a shared-placement run and either provide
the common typed permit through verified effective-writer quiescence or deny
that surface. Each supported operating-system backend must separately pass
process-tree, cancellation, restart, and quarantine evidence. ADR acceptance
alone will not qualify or enable shared placement.

If maintainers later reject the restricted boundary or require protection from
external same-user writers, keep shared placement unavailable and retain
isolated worktrees unless a separately qualified OS boundary is introduced.

## Consequences

- ADR-0002 remains effective and no placement default changes.
- M006 isolated launch and M007 build admission remain independent of M005.
- No writer-permit service, public placement selector, or shared-write path is
  authorized by this decision.
- M005 can proceed to per-platform complete-mediation feasibility; Q1 remains
  gated on the writer inventory and deny matrix. Unsupported persistent
  capabilities remain unavailable.
- M008 S4 remains gated on M005 Q1–Q5; this ADR does not unblock it.

## Supersession

None. This decision does not change ADR-0002 or accepted ADR-0032.
