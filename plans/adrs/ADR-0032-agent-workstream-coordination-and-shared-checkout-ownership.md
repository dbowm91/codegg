# ADR-0032: Daemon-Owned Agent Workstream Coordination and Explicit Shared-Checkout Mutation

Status: accepted — architecture direction accepted 2026-10-15; shared-checkout execution remains unauthorized until M005 qualification

Date: 2026-10-15

Decision owners: project maintainers

Related specification sections:

- plans/000-long-term-specification.md §§4, 6, 9, 16, 17, 26, 27, 29
- plans/001-terminology-and-domain-model.md §§5, 7, 8, 12, 13
- plans/002-long-term-roadmap.md phases 9–11
- plans/003-planning-process.md §§2, 4, 6

Affected subsystem roadmap: plans/subsystems/agent-workstream-blackboard-roadmap.md

Existing authorities: ADR-0001 (Tool Program authority); ADR-0002 (durable AgentRun/worktree isolation); ADR-0004 (approval/sandbox); ADR-0005 (WorkOrders); ADR-0031 (authenticated idle continuation).

## Context

CodeGG has scheduler-owned durable AgentRun/AgentRunGroup records, a bounded run-control mailbox and journal, worktree leases, session projections, typed Git integration, WorkOrders and background jobs. Its production SubagentExecutor currently allocates a separate managed worktree for every mutation-capable durable child; read-only children may inherit the checkout. SessionStore::fork copies conversation history and workspace binding without allocating a new checkout. WorkOrders separately define AutoIsolated/Shared/Serialized, but ordinary task spawn has no placement selector or group-owned checkout semantics.

Independent worktrees isolate file/index state but cannot stop duplicate objectives, stale interface assumptions or resource contention. The inverse—multiple unsupervised writers in one checkout—risks lost edits and uncontrolled Git-index mutations. We need typed shared observation and task ownership independently from filesystem placement.

Research motivation, not a product performance guarantee: Google Research's 2026 agent-scaling evaluation reports large task-dependent differences between centralized and independent agents; ESEM 2026 PASC finds action-space peer observations beneficial for two agents on a specific Python repair benchmark; CooperBench team-trajectory datasets favor structured task lists over plain cooperative Git in one experimental harness. These studies use different workloads and denominators. CodeGG must measure its own cost/success trade-offs, including Rust builds, before claiming a universal default.

## Decision drivers

- One daemon/scheduler, one durable agent-run service, one canonical Git/worktree service; no second blackboard daemon or orchestrator.
- Typed project, workspace, repository, worktree, group, task and run identities; paths never create authority.
- Useful peer awareness without sibling chat, hidden reasoning exposure, untrusted prompt injection or excessive model tokens.
- Atomic responsibility claims and leases for actual shared-write safety, with explicit ownership transfer on overlap.
- Allow single-agent, separate-worktree and serialized shared-checkout execution, with conservative default isolation.
- Crash/restart/idempotency, permission ceilings, tenant isolation, bounded retention and replay.

## Alternatives

### A — Prompt-only task division and sibling chat

Minimal code, but no authoritative claims, provenance, conflict prevention, recovery or privacy boundaries. Rejected.

### B — Git-tracked scratchpad or append-only custom Git refs as sole authority

Portable across checkouts but invites Git/index/merge contention, uncontrolled agent writes, workspace-local state drift and redundant durability. Useful as an optional export/interoperability format in the future, not the on-prem daemon authority. Rejected.

### C — Derive a read-only, group-scoped blackboard from canonical run/job/Git state; add small CAS-owned responsibility records and optional serialized shared-checkout group ownership

Selected and accepted as the architecture direction, subject to M005
qualification before any changed lease or shared-write behavior is enabled.

## Decision

Accepted by explicit user direction on 2026-10-15. This accepts the
architecture direction below; it does not qualify or enable serialized
shared-checkout execution. M005 must still prove effective writer-path
containment and satisfy the verification gates below before any shared-write
mode can be activated.

1. Define an AgentWorkstream as a bounded coordination view over an existing AgentRunGroup or direct owner and its authorized members; it is NOT a new scheduler, job, WorkOrder, session, execution authority or globally writable document. Reuse AgentRunGroupId where practicable; add a distinct ID only if tests prove groups cannot express required lifecycle.
2. The daemon owns a durable, versioned workstream snapshot. Observed facts (run status, Git commits/changed paths, job/test status, dependency completion) are derived from canonical services. Agent-authored intent (task descriptions, declared intended paths, blockers, handoffs) is stored separately, labeled as claims, and never masquerades as verified execution evidence.
3. Writes use attributed actor authority and atomic compare-and-swap/version checks. Ownership may be claimed, released, handed off or reallocated only under authorization; knowledge of a group ID is not authorization. Sibling agents may read a bounded, scoped projection, but may not steer/control each other. Parent/group-owner messages continue through existing run-control authority.
4. Deliver peer observations at safe agent-loop boundaries as bounded, deduplicated, relevance-filtered snapshots. Each snapshot has an authoritative revision and a source/evidence class. Detailed state is available on demand; no unbounded transcript streams, continuous model polling or automatic wake solely for progress.
5. Placement is independent of awareness: single agent, isolated managed worktree (default for mutating child), existing authorized read-only checkout, or explicit serialized shared checkout. Group checkout ownership is separate from participating run access; each participant receives scoped execution rights. Preserve ADR-0002 automatic isolation until explicit opt-in, accepted semantics and proof.
6. In the first shared-checkout implementation, mutation-capable members may reason and perform safe reads concurrently, but all file/index/Git-mutating operations are serialized under a checkout-scoped scheduler/execution guard. Do NOT infer disjoint-write safety from file path claims; arbitrary shell, terminal, plugin, LSP and subprocess mutations must respect the same effective boundary or shared mutation must fail closed. True simultaneous disjoint writers are deferred to a separate decision.
7. A workstream never increases tool, sandbox, approval, path, network or Git privileges. An existing checkout may be shared only with an explicit authorized reservation and measured fallback; default to isolated worktrees if uncertain.
8. Separate build-resource coordination from placement. Reuse existing scheduler Job/Attempt/RunStore authority and support optional per-workspace compilation limits; no automatic universal shared CARGO_TARGET_DIR or hidden environment widening.
9. No implicit integration of child commits. The existing typed Git integration remains the source of mutation/recovery authority. Integration of multiple results on an advancing parent requires explicit preconditioned policy and tests.
10. Session fork and worktree allocation remain distinct atomic objects; a combined user operation must be a recoverable daemon orchestration with idempotency and no partial invisible checkout/session.

## Consequences and tradeoffs

Benefits: less duplicate discovery/editing, explicit ownership, structured peer visibility across isolated or shared checkouts, deterministic contention and better control over build amplification.

Costs: a small storage/migration and projection surface; coordination tokens; CAS races; run/group ownership semantics; cross-platform process/sandbox qualification; potential throughput regression from serialized writes. A blackboard is NOT a substitute for filesystem/sandbox isolation.

## Compatibility and migration

M001–M004 can add read-only observation and durable coordination without changing ADR-0002's existing mutation isolation contract. Explicit shared mutation and group-owned worktrees (M005) MUST NOT ship until this ADR is accepted, all mutation-capable tool surfaces have a common enforceable containment/serialization path, and an opt-in remains the only activation path. Legacy SubAgentPool requests and old AgentRunGroup rows retain current isolated defaults; no historical transcript rewrites. Storage/protocol extensions are additive, bounded and versioned.

Once accepted and qualified, only ADR-0002's unconditional *per-child checkout for concurrently mutation-capable descendants* is narrowed for explicitly serialized, group-reserved checkout execution; all other ADR-0002 ownership, authorization and recovery requirements stay in force. Until acceptance, no supersession occurs.

## Security, reliability and stop conditions

- Enforce project/principal/run/group authorization server-side and tool execution side; never trust agent-authored blackboard text as instruction or source of truth.
- Redact credentials, raw commands, tool arguments, messages, transcripts, hidden reasoning, arbitrary absolute paths and unbounded stderr from peer projections.
- Monotonic sequence/CAS, deduplication keys, generation-fenced leases, stale actor rejection, bounded TTL/retention, restart reconciliation, cancellation and orphan recovery.
- Respect private projects and cross-principal tenancy; group membership is not a blanket permission grant.
- Stop shared-write enablement if any Bash/terminal/exec/plugin/LSP/file/Git mutation path can circumvent the group execution reservation or if an approval/sandbox ceiling would need widening. In that case retain separate worktrees and deliver only peer-awareness improvements.

## Verification required before shared-write enablement and strict ADR closure

1. Contract review of existing ADR-0002, WorktreeService/lease generation, SubagentExecutor, WorkOrders and all mutation paths.
2. Two agents racing for one responsibility, stale/replayed writes, overlap/rename/symlink claims, cancelled owners, restarts and 16-member capacity limits.
3. Permission/tenant isolation, provenance redaction, prompt-injection test corpus, transport-bounded snapshots and zero sibling control escalation.
4. Non-Git shared mutation and unqualified platform backends fail closed; no misleading 'isolated' or 'serialized' labels.
5. Rust toolchain build contention measured separately from Git checkout contention; tests quantify CPU, peak disk, compilation wall time.
6. Compare single strong agent to independent worktrees, peer-aware isolated worktrees, claimed isolated groups and serialized shared groups on equivalent tasks/budgets. No unqualified improvement claims.
7. Existing focused tests, guards, quick verification and applicable hosted CI recorded as actual evidence.

## Supersession

Proposed limited clarification of ADR-0002 condition #7 only after explicit acceptance and qualified implementation. ADR-0002 is not modified or superseded by filing this proposal.
