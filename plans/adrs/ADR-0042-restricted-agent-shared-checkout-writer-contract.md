# ADR-0042: Restricted CodeGG-Managed Shared-Checkout Writer Contract

Status: proposed — NOT accepted; does not authorize shared-checkout mutation

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
**Proposed selection, pending explicit maintainer acceptance.** Supports opt-in checkout coordination among CodeGG-managed operations only. It is not an external-writer exclusion guarantee or sandbox.

### C — Advisory path claims plus prompt instructions
Rejected; not an enforceable file/index mutation boundary.

## Proposed decision

1. **Boundary:** Coordinate CodeGG-initiated mutations to an explicitly reserved canonical checkout across all agents, jobs, daemon APIs and authenticated human CodeGG operations. Unrelated editors, shells and processes launched outside CodeGG by the same OS user are NOT included. If their exclusion is required, this profile is not usable; a separate OS isolation ADR is required. User-facing labels must not say exclusive, sandboxed, or filesystem isolated.
2. **Default:** Existing mutating-agent isolated worktree placement continues. An explicit serialized group reservation requires owner/project/principal authorization and the restricted capability profile. No model/client-provided path, WorktreeId, generation or group ID is a bearer credential. Missing or ambiguous scope refuses before an effect.
3. **Initial profile:** Native checked file transactions and typed Git writes are only *candidate* enabled mutations, subject to complete interposition. All unqualified shell, Python, test/build/formatter process, detached Tool Program, PTY, LSP/MCP server, process-backed plugin/hook and other process-capable tool routes are denied before launch; read-only calls must not start unqualified persistent processes. Non-Git mutable shared execution remains unsupported. Reads and model reasoning can overlap.
4. **All CodeGG sinks:** Direct document/editor writes, LSP preview application, HTTP file mutations, project initialization, snapshot restore, project-local state and other non-broker sinks must join the same permit or fail before effect. Authenticated human CodeGG writes may not bypass it. Claims are advisory; no path-glob locking as a safety substitute.
5. **Durable owner:** Resolve a typed tuple of WorktreeId, worktree lease generation, group reservation, authenticated actor/run, and execution attempt. One daemon-owned CAS/fenced writer permit owns the complete effect lifetime. A request timeout, failed output join or expired lease is not proof of quiescence. Ambiguous restart/cancellation quarantines the checkout until resolved; no automatic release while a writer may continue. No permit held during user approval/provider inference/child scheduling.
6. **Processes:** Native process-backed shared mutation requires separately qualified descendant-tree quiescence on each supported OS; no inference from direct-child exit alone. Later process extensions require their own bounded implementation and qualification, not silent widening of profile v1.
7. **Authority and surfaces:** Resolve execution identity through daemon records and enforce the profile at the final mutation/launch owner, after pre-approval and after policy narrowing. Unsupported operations, compatibility adapters and non-broker APIs must refuse before mutation. Existing tool/sandbox/Git/network permissions remain independent and cannot be widened by the shared profile.
8. **Truthful UX:** Expose “CodeGG-mediated serialized checkout” with exact unsupported operations and quarantine/attention state, not “all writers prevented”. No privileged sibling control, hidden reasoning or secrets in projections.

## Consequences and tradeoffs

A small safe initial surface is plausible and testable but intentionally excludes arbitrary compilation/test commands inside shared checkout until native containment exists. For full coding-tool breadth retain isolated worktrees and independently prioritize M006/M007. External same-user edits can conflict; the guarantee does not prevent them.

## Compatibility and migration

Proposed clarification of the qualified operating boundary under accepted ADR-0032; it does not revise accepted ADRs or enable execution on its own. Additive versioned schema/protocol changes, opt-in only, legacy isolated behavior unchanged. If ADR-0042 is rejected, C002–C006 remain blocked and M005 shared mutation stays unavailable.

## Security, reliability, qualification and stop

Threat model: stale generations, forged identities, restart/panic, concurrent separate CodeGG API writes, symlink/ref aliasing, approval/lock deadlock, descendant escape, unsupported platform and independent same-user writers (explicitly outside scope). Q1–Q5 must prove serial native writer effects, denied bypasses, idle/restart quarantine, genuine two-run Git integration and an exact platform capability matrix. No universal containment claim from mocks, process-group tests or hosted compilation.

## Supersession

None. Accepted ADR-0002 isolation and accepted ADR-0032 architecture remain in force. Filing this proposed decision does not itself alter either contract.
