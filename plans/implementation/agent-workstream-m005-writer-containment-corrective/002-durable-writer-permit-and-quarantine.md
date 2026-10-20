# M005 Writer-Containment Corrective C002 — Durable Fenced Checkout Writer Permit (Q1)

Status: closed — closure record `plans/closure/agent-workstream-m005-writer-containment-corrective/002-status.md`
Primary class: infrastructure / invariant
Planning baseline: plan/agent-workstream-m005-writer-audit (2026-10-19 source audit; revalidate at execution).
Source: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md C002.
Original: plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md, Q1.
Accepted predecessor decisions: ADR-0002, ADR-0032, **ADR-0044 (option B, accepted 2026-10-19)**. The ADR-0044 hard gate is satisfied; it authorizes the C002 permit service and nothing else.

## Objective

Implement *only* the reusable authoritative, generation-fenced permit and lifecycle service for a registered managed worktree that has an explicitly authorized shared reservation. This is the single CodeGG-checkout writer token across runs and non-broker services, NOT a model tool, admission scheduler, project-level global mutex, or an inference that any particular writer has been mediated. No shared mutation may be activated in this milestone.

## Ownership and contract

Resolve typed ProjectId, RepositoryId, WorkspaceId, WorktreeId, current WorktreeService lease generation, AgentRunGroup owner, authenticated actor/AgentRunId, and owning job/attempt where present from daemon stores. Only the daemon creates an unforgeable operation context. Keep worktree checkout *owner* distinct from current *writer permit owner*. A group reservation is not general principal access. Existing WorktreeService lease mechanics remain authoritative for physical checkout health and dirty retention; design additive data only for write serialization.

## Work packages

A. Define explicit states Available, Held(owner, generation, attempt, operation), Releasing, Quarantined(reason). On acquisition verify active group reservation, current lease generation, project/principal/run auth and narrowed effective authority. Reject unknown/stale IDs. Add a single-owner transactional CAS with monotonic revision and idempotent invocation handling; versioned schema/migration if persistence is necessary.

B. Provide bounded async waiter API integrated with the existing scheduler cancellation semantics but no second resource admission/queue. Distinct worktrees proceed independently; no global worktree-mutation key as the safety gate. Waiters cannot starve other groups indefinitely; cancellation/permission wait must not leak a writer token. No permit held during provider inference, approval or submission of a nested job that needs the same permit. Nested reuse only with explicit identity-bound non-duplicable final-effect owner transfer.

C. Define normal release only after the effective checked transaction is complete and rollback/commit has resolved. A process-backed caller must provide positive quiescence evidence before release (unqualified process callers are denied in later milestones). On aborted operation, incomplete effects, process-tree uncertainty, daemon crash or expired lease, leave Quarantined until explicit authoritative recovery. Lease deadline alone is NEVER enough to free a live writer.

D. Restart reconciliation: persisted orphan owner/attempt, active scheduler job state, worktree health and lease generations; do not replay a non-idempotent mutation to repair metadata. Typed operator inspect/release only after verified quiescence; never silently delete dirty files. Expose bounded event/projection data with redaction.

E. Core boundary: codegg-core typed IDs/store/state machine, root daemon adapter supplies authorization, WorktreeService/JobScheduler coordination; no codegg-core dependencies on TUI/server/tool impl. Separate permit from scheduling capacity and native tool authorization.

## Required qualification

Deterministic two-run N=2 and N=16 CAS contention (one holder), different-worktree parallelism, stale owner/generation/ABA, duplicate retry with altered payload, cancellation while waiting, panic/transaction rollback, expiry and restart, release-after-worktree ownership change, cross-principal/project denial, hard stop/quarantine when process completion unproven. Repeat with SQLite close/reopen and competing transactions; verify no held permit while waiting for job admission. Confirm no user-visible shared-mutation selector activated.

Commands: cargo test -p codegg-core [focused module], cargo test -p codegg [root integration], cargo fmt --all -- --check, bash scripts/check-core-boundary.sh, python3 scripts/check_scheduler_bypass.py, scripts/verify.sh quick, relevant authorization/approval guards, workspace Clippy when available. Record exact names and results.

## Acceptance / stop / closure

Q1 is closed only with proven generation-fenced *store and daemon consumer*, deterministic persistence/restart quarantine and race/tenant tests. C002 is not shared-write readiness. Stop on lease expiry automatic release, bypassable actor IDs, second scheduler, held-permit wait deadlock, unversioned schema or need to expose a bearer token to models. Write plans/closure/agent-workstream-m005-writer-containment-corrective/002-status.md and unblock C003 only after strict closure.

## Delivered by

- **Authority (codegg-core):** `crates/codegg-core/src/writer_permit.rs` — the
  typed scope, the four-state machine, the durable store with its
  single-owner CAS under a monotonic revision, the idempotency binding, the
  evidence-gated release, the quarantine transitions, the operator recovery,
  and the restart reconciliation.
- **Shared write transaction:** `crates/codegg-core/src/storage/immediate.rs`
  — the `BEGIN IMMEDIATE` primitive, extracted out of the user-prompt queue
  store so both use one mechanism. `scripts/check_queue_write_transaction_shape.py`
  follows it and still proves it rejects a deferred read-then-write path and a
  closing rollback.
- **Schema:** `migrate_v84` — the additive `writer_permit` table;
  `STORAGE_LAYOUT_VERSION` 83 → 84.
- **Daemon adapter:** `src/core/writer_permit.rs` — the authority resolution
  over the worktree/group/team stores, the bounded per-checkout waiter, the
  `WriterPermitGuard` holder contract, and the redacted operator surface.
- **Wiring:** `src/core/runtime_deps.rs` (the daemon owns exactly one permit
  service), `src/core/daemon_construct.rs` (startup reconciliation).
- **Qualification:** `crates/codegg-core/tests/writer_permit.rs` (21 tests)
  and `tests/writer_permit_service.rs` (12 tests).
- **Documentation:** `architecture/writer_permit.md`, `architecture/overview.md`,
  `.opencode/skills/scheduler/SKILL.md`.

**What it did not do.** No writer is routed through the permit. No shared-write
mode, no user-visible shared-mutation selector, and no user-facing enablement
exists; the isolated worktree remains the default placement for mutating
agents. Routing writers is C003 and C004.
