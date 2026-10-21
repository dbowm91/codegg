# ADR-0047: Restricted CodeGG-Managed Shared-Checkout Writer Contract

Status: accepted — option B selected 2026-10-19; does not by itself authorize shared-checkout mutation

Date: 2026-10-19

Decision owners: project maintainers

Source architecture:
- plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md (accepted)
- plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md (accepted direction; M005 gate remains)
- plans/000-long-term-specification.md §§9, 16–17, 26–29
- plans/001-terminology-and-domain-model.md §§5, 7–8
- plans/003-planning-process.md
- plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md

Affected roadmap: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md

## Context

The M005 audit (2026-10-19) identifies native file and Git operations, scheduled shell/Python/Tool Programs, interactive PTYs, persistent LSP/MCP/plugin/hook processes, editor/document writes, HTTP file routes, project initialization, snapshot restore, and project-local state. ToolBatchExecutor serializes within a batch, not across runs. WorktreeService leases track checkout ownership, not all mutations. Scheduler exclusive keys are coarse and miss direct AgentTurn and non-broker writes. Unix children that call setsid can survive process-group teardown, and request completion does not prove a persistent process has stopped writing. External same-user editors do not participate in daemon locks. None of M005 Q1–Q5 is yet qualified.

## Decision drivers

- Preserve ADR-0002 automatic isolated worktrees and truthful status.
- Never confuse a daemon serialization protocol with an OS sandbox.
- A task/path claim, process-resource key, repository lock or Git lease is not write authority.
- All CodeGG-managed writers must participate in the same effective final writer boundary or refuse before effect.
- Separate capability feasibility from process isolation ambitions and avoid a second scheduler.

## Options

### A — Universal filesystem exclusion, including unrelated same-user processes
Requires separately qualified OS identity/filesystem containment on each platform. Process groups, Job Objects and daemon locks do not satisfy it. Deferred as a different security project.

### B — Restricted CodeGG-managed coordination, denying all unqualified execution surfaces
**Selected and accepted 2026-10-19.** Supports opt-in checkout coordination among CodeGG-managed operations only. It is not an external-writer exclusion guarantee or sandbox.

### C — Advisory path claims plus prompt instructions
Rejected; not an enforceable file/index mutation boundary.

## Decision

1. **Boundary:** Coordinate CodeGG-initiated mutations to an explicitly reserved canonical checkout across all agents, jobs, daemon APIs and authenticated human CodeGG operations. Unrelated editors, shells and processes launched outside CodeGG by the same OS user are NOT included. If their exclusion is required, this profile is not usable; a separate OS isolation ADR is required. User-facing labels must not say exclusive, sandboxed, or filesystem isolated.
2. **Default:** Existing mutating-agent isolated worktree placement continues. An explicit serialized group reservation requires owner/project/principal authorization and the restricted capability profile. No model/client-provided path, WorktreeId, generation or group ID is a bearer credential. Missing or ambiguous scope refuses before an effect.
3. **Initial profile:** Native checked file transactions and typed Git writes are only *candidate* enabled mutations, subject to complete interposition. All unqualified shell, Python, test/build/formatter process, detached Tool Program, PTY, LSP/MCP server, process-backed plugin/hook and other process-capable tool routes are denied before launch; read-only calls must not start unqualified persistent processes. Non-Git mutable shared execution remains unsupported. Reads and model reasoning can overlap.
4. **All CodeGG sinks:** Direct document/editor writes, LSP preview application, HTTP file mutations, project initialization, snapshot restore, project-local state and other non-broker sinks must join the same permit or fail before effect. Authenticated human CodeGG writes may not bypass it. Claims are advisory; no path-glob locking as a safety substitute.
5. **Durable owner:** Resolve a typed tuple of WorktreeId, worktree lease generation, group reservation, authenticated actor/run, and execution attempt. One daemon-owned CAS/fenced writer permit owns the complete effect lifetime. A request timeout, failed output join or expired lease is not proof of quiescence. Ambiguous restart/cancellation quarantines the checkout until resolved; no automatic release while a writer may continue. No permit held during user approval/provider inference/child scheduling.
6. **Processes:** Native process-backed shared mutation requires separately qualified descendant-tree quiescence on each supported OS; no inference from direct-child exit alone. Later process extensions require their own bounded implementation and qualification, not silent widening of profile v1.
7. **Authority and surfaces:** Resolve execution identity through daemon records and enforce the profile at the final mutation/launch owner, after pre-approval and after policy narrowing. Unsupported operations, compatibility adapters and non-broker APIs must refuse before mutation. Existing tool/sandbox/Git/network permissions remain independent and cannot be widened by the shared profile.
8. **Truthful UX:** Expose “CodeGG-mediated serialized checkout” with exact unsupported operations and quarantine/attention state, not “all writers prevented”. No privileged sibling control, hidden reasoning or secrets in projections.

## Acceptance record

Decision: **option B accepted**, 2026-10-19, by the project maintainers
(David Bowman), on branch `work/agent-workstream-m005-writer-audit`.

Provenance of the decision package:

- The decision was requested by the M005 writer-containment corrective C001,
  whose closure record is
  `plans/closure/agent-workstream-m005-writer-containment-corrective/001-status.md`
  and whose contract deliverable is
  `plans/implementation/agent-workstream-m005-writer-containment-corrective/c001-restricted-profile-contract.md`.
- That package enumerated 48 CodeGG writer roots with a declared final effect
  owner, checkout locator, authority path, lifetime, adversarial bypass example
  and a restricted-profile disposition, together with the manifest
  `docs/writer-disposition.toml` and the static inventory guard
  `scripts/check_writer_disposition.py`.
- C001 closed with the decision outcome "still proposed". This acceptance is the
  separate, independent maintainer decision that the C001 gate required; it is
  not a restatement of C001's own outcome.

What this acceptance does:

1. It selects option B as the restricted CodeGG-managed coordination profile and
   makes the eight numbered clauses above the governing writer contract for the
   M005 corrective line.
2. It unblocks **C002 only** — the durable, generation-fenced checkout writer
   permit and its lifecycle service. C003–C006 remain blocked on their own
   predecessor closures, exactly as the corrective's dependency graph states.
3. It records that option A (universal filesystem exclusion, including
   unrelated same-user processes) is **not** selected and is deferred to a
   separate OS-isolation project, and that option C (advisory path claims plus
   prompt instructions) is rejected as not being an enforceable mutation
   boundary.

What this acceptance does **not** do:

- It does not enable shared-checkout mutation. No shared-write mode exists, no
  writer permit exists, and no user-visible shared-mutation selector is
  activated. Per clause 3, every unqualified execution surface stays denied
  until the later milestones implement and qualify the denial.
- It does not amend accepted ADR-0002 (agent-run worktree isolation and
  control) or accepted ADR-0032 (workstream coordination and shared-checkout
  ownership). The isolated worktree remains the default placement for mutating
  agents, and the whole legacy isolated behavior is unchanged.
- It does not make CodeGG's guarantee a filesystem exclusion, a sandbox, or an
  exclusion of independent same-UID programs. Clause 1 and the user-facing
  labels in clause 8 remain binding: the label is "CodeGG-mediated serialized
  checkout", never "exclusive", "sandboxed" or "filesystem isolated".
- It does not qualify Q1–Q5. Those remain unrun until C002–C006 land, and M005
  strict closure still requires C006's evidence.

## Consequences and tradeoffs

A small safe initial surface is plausible and testable but intentionally excludes arbitrary compilation/test commands inside shared checkout until native containment exists. For full coding-tool breadth retain isolated worktrees and independently prioritize M006/M007. External same-user edits can conflict; the guarantee does not prevent them.

## Compatibility and migration

Accepted clarification of the qualified operating boundary under accepted ADR-0032; it does not revise accepted ADRs or enable execution on its own. Additive versioned schema/protocol changes, opt-in only, legacy isolated behavior unchanged. If ADR-0047 is rejected, C002–C006 remain blocked and M005 shared mutation stays unavailable.

*Superseded clause (historical).* This section was written while the decision
was proposed and read "Proposed clarification … If ADR-0047 is rejected, C002–C006
remain blocked". Option B was accepted on 2026-10-19, so the rejection branch is
now counterfactual; the "Accepted clarification" and "does not … enable execution
on its own" clauses are the operative ones. Had it been rejected, C002–C006
would have remained blocked and M005 shared mutation would have stayed
unavailable, and a rejection that still wanted independent same-user writer
exclusion would have required a separate OS-isolation ADR.

## Security, reliability, qualification and stop

Threat model: stale generations, forged identities, restart/panic, concurrent separate CodeGG API writes, symlink/ref aliasing, approval/lock deadlock, descendant escape, unsupported platform and independent same-user writers (explicitly outside scope). Q1–Q5 must prove serial native writer effects, denied bypasses, idle/restart quarantine, genuine two-run Git integration and an exact platform capability matrix. No universal containment claim from mocks, process-group tests or hosted compilation.

## Supersession

None. Accepted ADR-0002 isolation and accepted ADR-0032 architecture remain in force. Accepting this decision does not itself alter either contract, and it does not enable shared-checkout execution.
