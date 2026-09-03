# Agent Convergence M001 — Durable Convergence Cycle Foundation

Status: ready

Repository baseline: `1bee32578566cc6cdf4025002af781309d8f29f4`

Source subsystem roadmap:

- `plans/subsystems/agent-convergence-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/003-planning-process.md`

Applicable decisions and dependencies:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- agent-run/worktree M009 is closed;
- goal-verification M013 is closed;
- no new ADR is required because this milestone does not change scheduler authority, agent-run ownership, Git authority, authorization, or an external/public protocol. If implementation discovers that one of those boundaries must change, stop and register an ADR before proceeding.

Primary class: invariant / infrastructure

## 1. Objective

Create the smallest durable host-owned state model needed to represent an independent produce/verify convergence operation without launching or scheduling any new agent from that state model.

M001 establishes the durable nouns, bounded task specification, state transitions, idempotency, evidence packet, persistence, and restart semantics that M002 will consume. The key design constraint is separation of concerns:

```text
ConvergenceService
    owns lifecycle/cycle records and validation

AgentRunStore / AgentRunGroupService
    own child execution identity and group coordination

Scheduler
    owns admission and resources

WorktreeService
    owns worktree leases

GoalVerificationService
    owns goal-completion certification
```

The convergence service must not grow hidden execution authority merely because later milestones will use it to coordinate runs.

## 2. Explicit non-goals

M001 must not:

- expose a model-facing `converge` action;
- spawn producer, verifier, repair, or replan agents;
- add a verifier built-in agent;
- run tests, read files, create worktrees, or integrate Git branches;
- change `AgentRunGroupService` join semantics;
- change `GoalVerificationService` or add LLM evidence as host completion authority;
- add model-profile orchestration flags;
- add a general workflow/DAG engine;
- touch the legacy file-backed team inbox/outbox implementation;
- add a new CI workflow or heavyweight verification gate.

If the milestone cannot be implemented without any of those changes, stop and record why rather than silently widening scope.

## 3. Current implementation evidence

The implementation agent must re-inspect these surfaces before editing:

- `crates/codegg-core/src/agent_run_group.rs` — durable group owner, membership, join, notification, and SQLite/in-memory storage. It explicitly does not admit work or own execution.
- `crates/codegg-core/src/agent_run.rs` — durable task/run records and structured run-result persistence.
- `crates/codegg-core/src/run_result.rs` — bounded `AgentRunResult`, validation, finding, artifact, commit, and repository-state contract.
- `src/agent/run_control.rs` and core run-control domain — exact turn/run owner authorization and stable-boundary control journal.
- `crates/codegg-core/src/goal/verification.rs` — host-owned deterministic completion boundary that semantic convergence must not replace.
- `crates/codegg-core/src/session/schema.rs` and storage-layout/migration tests — canonical SQLite migration ownership.
- session projection types/reducers — additive bounded state projection conventions.

Do not infer the final API from this plan if current names have changed. Preserve the ownership boundary and use the current canonical module names.

## 4. Required domain contract

### 4.1 Convergence identity and owner

Add a typed, bounded convergence handle in `codegg-core`. It may be a new `ConvergenceId`/`AgentConvergenceId` or a service-local typed wrapper around an existing opaque ID if that avoids unnecessary global terminology expansion.

Requirements:

- stable across restart;
- parse/serialize/display validation consistent with existing typed IDs;
- not derived from a filesystem path or display title;
- not used as a replacement for `AgentRunId`, `AgentRunGroupId`, `JobId`, `TurnId`, or `GoalId`;
- never grants authority by possession alone.

Represent the owner with the existing orchestration ownership shape where possible:

```text
Turn { session_id, turn_id }
Run { run_id }
```

Do not invent a fake root `AgentRun` for a turn-owned convergence.

### 4.2 Durable bounded task specification

A detached/restarted convergence must not reconstruct its purpose or acceptance criteria from model transcript prose. Persist the exact convergence-specific bounded objective and criteria (or persist an existing durable bounded artifact reference carrying that exact specification) as host-owned structured state.

Preferred direct shape:

```rust
struct ConvergenceSpec {
    objective: String,
    criteria: Vec<String>,
    objective_digest: String,
    criteria_digest: String,
}
```

Recommended initial hard bounds:

```text
objective <= 8 KiB
criteria count <= 32
each criterion <= 1 KiB
```

Implementation may choose tighter limits. This is not permission to persist the user's entire conversation or raw system prompt. The convergence spec contains only the explicit delegated objective/acceptance criteria accepted at convergence creation.

Digests support request fingerprinting/audit and do not replace the durable text needed to construct a verifier after restart.

### 4.3 Convergence state

Define a host-owned state enum covering at least:

```text
pending
producing
verifying
awaiting_decision
repairing
replanning
completed
failed
cancelled
exhausted
```

M001 may not exercise every state, but the transition function must encode the legal graph so later milestones do not use free-form status strings.

Terminal states are monotonic. No transition may move a terminal record back to active.

Recommended transition skeleton:

```text
Pending -> Producing | Cancelled
Producing -> Verifying | Failed | Cancelled
Verifying -> AwaitingDecision | Failed | Cancelled
AwaitingDecision -> Completed | Repairing | Replanning | Failed | Cancelled | Exhausted
Repairing -> Verifying | Failed | Cancelled | Exhausted
Replanning -> Producing | Failed | Cancelled | Exhausted
```

M001 tests the complete graph even though M002/M003 are the first production consumers.

### 4.4 Convergence record

Persist bounded structural state equivalent to:

```rust
struct ConvergenceRecord {
    id: ConvergenceId,
    owner: AgentOrchestrationOwner,
    spec: ConvergenceSpec,
    status: ConvergenceStatus,
    current_cycle: u8,
    max_cycles: u8,
    created_at: i64,
    updated_at: i64,
    terminal_at: Option<i64>,
    revision: u64,
    idempotency_key: String,
}
```

Hard limits must include:

- maximum idempotency-key bytes;
- maximum cycles with a code-level hard ceiling no greater than 4 initially;
- objective/criterion count and byte limits;
- digest/string lengths;
- bounded serialization size for any JSON fields.

### 4.5 Cycle record

Persist one row/record per cycle ordinal with references, not copied execution state:

```rust
struct ConvergenceCycleRecord {
    convergence_id: ConvergenceId,
    ordinal: u8,
    producer_group_id: Option<AgentRunGroupId>,
    producer_run_ids: Vec<AgentRunId>,
    verifier_run_id: Option<AgentRunId>,
    verdict: Option<SemanticVerificationVerdict>,
    decision: Option<ConvergenceDecision>,
    source_base_commit: Option<String>,
    result_commit: Option<String>,
    created_at: i64,
    completed_at: Option<i64>,
}
```

If the schema is cleaner with normalized cycle-member rows rather than a JSON vector, use the existing group/run ownership patterns. Do not duplicate complete `AgentRunResult` values in convergence storage; reference run IDs and re-read the authoritative result store.

### 4.6 Semantic verdict and decision types

Define bounded serializable types, approximately:

```rust
enum SemanticVerificationVerdict {
    Pass {
        summary: String,
        evidence_refs: Vec<String>,
    },
    Revise {
        findings: Vec<AgentRunFinding>,
        repair_requests: Vec<String>,
    },
    Inconclusive {
        reason: String,
        missing_evidence: Vec<String>,
    },
}

enum ConvergenceDecision {
    Accept,
    Repair,
    Replan,
    Stop,
    Escalate,
}
```

Bounds should reuse `run_result` limits where semantically appropriate rather than creating larger parallel limits.

The verdict type must carry a prominent code/documentation invariant: `Pass` is semantic/advisory and is not equivalent to `GoalVerificationVerdict::Met`.

## 5. Verifier evidence packet

Implement a pure assembler that converts authoritative existing state into a bounded verifier packet. M001 does not send it to a model.

Input should be the persisted `ConvergenceSpec`, one or more authoritative `AgentRunResult` values, and optional existing artifact/diff references. Output should contain only fields needed for an independent reviewer, such as:

```rust
struct VerifierEvidencePacket {
    objective: String,
    criteria: Vec<String>,
    producer_runs: Vec<ProducerEvidence>,
    base_commit: Option<String>,
    result_commit: Option<String>,
    changed_paths: Vec<String>,
    validation: Vec<ValidationEvidence>,
    findings: Vec<AgentRunFinding>,
    artifacts: Vec<AgentRunArtifact>,
    repository_state: RepositoryState,
}
```

Requirements:

- explicit per-field and envelope bounds;
- no producer transcript, hidden reasoning, tool arguments, environment variables, credentials, or raw unbounded tool output;
- invalid/missing run result produces a typed `MissingEvidence`/`Inconclusive` assembly error rather than an empty success packet;
- failed/cancelled/conflicted producer status remains visible and cannot be normalized to success;
- artifact handles remain handles unless an existing bounded artifact-read API is explicitly called by a later verifier run;
- detached/restart assembly uses the persisted convergence spec, never a best-effort scrape of session transcript text.

The assembler should be deterministic for identical authoritative inputs. Add a digest/fingerprint if useful for idempotency and audit, but do not make the digest itself proof that the evidence is correct.

## 6. Store and migration

### 6.1 Store interface

Add an in-memory implementation for tests and a SQLite-backed production implementation following existing core store conventions.

Required operations should cover:

- `create_or_get` with idempotency/fingerprint validation over the bounded convergence spec and owner;
- get by convergence ID;
- list by exact owner with a bounded result count;
- compare-and-set/revision-checked transition;
- create/get cycle by `(convergence_id, ordinal)`;
- set producer/group references exactly once or through a revision-checked legal transition;
- set verifier run exactly once for a cycle;
- set verdict and owner decision with first-valid-transition semantics;
- list nonterminal records for restart reconciliation with a hard bound/pagination pattern consistent with other stores.

Do not add generic arbitrary update methods that let callers bypass the state machine.

### 6.2 SQLite schema

Use the repository's next storage migration. The exact table names may follow conventions, for example:

```text
agent_convergence
agent_convergence_cycle
agent_convergence_cycle_member   # only if normalized producer membership is needed
```

The convergence table must durably store the bounded objective/criteria or a stable reference to an existing durable bounded artifact containing them. Do not store only digests if no authoritative text survives restart.

Indexes should support demonstrated queries only: primary ID, owner/status recovery, and unique idempotency key. Avoid speculative analytics indexes.

Migration requirements:

- restart-safe and idempotent through the existing schema version mechanism;
- old databases open without a manual data migration step;
- no rewrite of existing run/group/goal/worktree rows;
- migration tests cover upgrading from the immediately previous supported layout;
- storage layout/version docs updated if required by repository convention.

## 7. Idempotency and stale-write rules

Creation must distinguish retry of one accepted invocation from a distinct identical request. Reuse the accepted invocation identity scheme already used by `TaskTool` where the convergence request is model-originated later; M001 can expose a caller-supplied bounded idempotency key and request fingerprint.

Rules:

- same key + same bounded request fingerprint/spec returns the original record;
- same key + different request fingerprint/spec fails closed;
- cycle ordinal is unique within a convergence;
- producer/verifier references cannot be replaced after a later state has consumed them;
- verdict cannot be overwritten by a retry with different contents;
- owner decision is revision-checked and first-valid-transition wins;
- cancellation racing with a decision results in one legal terminal/active state according to transaction order, never both side effects.

## 8. Restart reconciliation contract

Add a pure reconciliation classifier that examines a nonterminal convergence record/cycle plus existing authoritative run/group status and returns one of:

```text
NoChange
AdvanceToVerifying
AdvanceToAwaitingDecision
MarkFailed
MarkCancelled
NeedsExecutionResume { phase }
NeedsAttention { reason }
```

M001 must not itself schedule missing execution. It records what the next owning application service should do after M002 exists.

Examples:

- record says `producing`, producer group is terminal successful -> classifier says `AdvanceToVerifying`;
- record says `verifying`, verifier run is terminal and verdict already persisted -> `AdvanceToAwaitingDecision`;
- record says `verifying`, verifier run terminal but no parseable/persisted verdict -> `NeedsAttention` or failure, not pass;
- record says `producing`, all producer runs failed/cancelled -> `MarkFailed`/`MarkCancelled` according to exact outcomes;
- terminal convergence -> `NoChange`.

Reconciliation must be idempotent across repeated daemon starts. Tests must also prove the bounded objective/criteria remain available after reload so M002 could resume verifier construction without transcript access.

## 9. Projection seam

Add only the DTO/reducer seam needed for later UI consumption, not a full TUI feature. A bounded summary should expose:

```text
convergence_id
owner summary
status
cycle ordinal / max cycles
producer run ids/status counts
verifier run id/status when present
verdict class only plus bounded summary
awaiting_decision bool
terminal reason class
```

Do not put full objective/criteria, findings, or diffs into the normal projection. Detailed convergence spec/evidence is fetched on demand through authorized detail operations.

If the current projection version can accept an additive optional collection/event without schema break, use it. Otherwise M001 may keep the core projection adapter internal and M002 can publish it when there is a user-visible consumer. Do not force a broad protocol version bump merely for unused M001 infrastructure.

## 10. Expected production-code touch set

Expected core surfaces:

- new `crates/codegg-core/src/agent_convergence.rs` or equivalent bounded module;
- `crates/codegg-core/src/lib.rs` exports;
- `crates/codegg-core/src/session/schema.rs` and migration/storage tests;
- session store construction/wiring needed to instantiate the SQLite convergence store;
- projection DTO/reducer files only to the extent described above.

Expected documentation:

- `architecture/agent.md` — add the convergence domain/authority boundary and durable spec;
- `architecture/storage.md` or canonical schema docs if table counts/layout are documented there;
- `architecture/overview.md` if a dedicated architecture page is introduced.

Do not touch producer/verifier prompts, built-in agents, `TaskTool` action schema, model profiles, or worktree integration in M001.

## 11. Required tests

### State machine

Cover every valid transition and representative invalid/stale transitions. Include terminal monotonicity and cycle-bound enforcement.

### Store/spec durability

For both in-memory and SQLite stores:

- create/get/list;
- objective/criteria bounded round trip;
- detached/restart reload preserves exact accepted spec;
- idempotent retry;
- same idempotency key with changed objective/criteria fails conflict;
- cycle ordinal uniqueness;
- revision/CAS race between two decisions/transitions;
- terminal transition cannot be reopened;
- bounded owner listing/recovery query;
- serialization limits.

### Migration

- open previous schema and migrate;
- new tables/indexes exist;
- existing agent-run/group rows remain untouched;
- repeated startup does not duplicate records.

### Evidence packet

- successful producer result preserves validation/findings/commit state;
- persisted convergence objective/criteria appear exactly within configured bounds;
- failed/cancelled/conflicted status cannot become pass-like evidence;
- oversized paths/findings/artifacts are bounded consistently;
- transcript/tool-output text is not present;
- missing result fails closed;
- deterministic identical input produces identical bounded packet/digest.

### Reconciliation

Table-driven cases for producing/verifying/awaiting/terminal records against producer/verifier run outcomes, including missing references and partially persisted state. Include restart assembly with no transcript access.

## 12. Verification commands

Required focused verification after implementation:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core agent_convergence --locked
cargo test -p codegg-core run_result --locked
```

Run the repository's canonical migration/storage test target if it is separate from the filters above.

Then run:

```bash
scripts/verify.sh quick
```

Do not add a new CI lane. Hosted CI is not required unless the exact implementation candidate already has an attributable hosted failure or the repository planning policy requires platform evidence for a changed supported-platform boundary.

## 13. Acceptance criteria

M001 may close only when all are true:

1. A durable convergence record has one exact turn/run owner and bounded idempotency identity.
2. The exact bounded convergence objective/criteria are durable and survive restart without reconstructing them from chat/model transcript prose.
3. A durable cycle record references producer group/runs and verifier run without copying authoritative run state.
4. The host state machine rejects illegal/stale transitions and terminal reopening.
5. Maximum cycles have a code-level hard ceiling of four or lower.
6. A typed semantic verdict and owner decision are persisted with explicit bounds.
7. `Pass` is documented/tested as advisory and cannot call goal completion.
8. Verifier evidence is assembled from the durable convergence spec plus bounded authoritative run-result fields without complete transcript/tool-output propagation.
9. SQLite migration and in-memory/SQLite store behavior are covered.
10. Restart reconciliation is deterministic and does not schedule or repeat work in M001.
11. No new scheduler, worker pool, worktree manager, permission authority, or team inbox is introduced.
12. Architecture/storage documentation is current.
13. Focused tests and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Stop and register a follow-up/ADR rather than broadening M001 if:

- implementing convergence requires changing scheduler admission semantics;
- a durable convergence handle must become a new canonical cross-subsystem identity in `plans/001-terminology-and-domain-model.md`;
- correct restart behavior requires replaying non-idempotent producer/verifier execution from convergence storage;
- `AgentRunResult` lacks a required authoritative evidence class that would require redesigning run-result ownership;
- durable objective/criteria cannot be stored safely without a new general prompt/transcript store;
- projection changes require a breaking protocol version rather than an additive seam;
- implementation starts adding a workflow language or raw transcript store.

## 15. Closure evidence required

Create `plans/closure/agent-convergence/001-status.md` containing:

- implementation commit(s);
- requirement-to-evidence matrix for every acceptance criterion;
- exact migration/storage version and tests;
- objective/criteria durability and restart evidence;
- state-machine/idempotency/reconciliation test results;
- evidence-packet redaction/bounds evidence;
- architecture/protocol compatibility review;
- focused verification and quick verification outputs;
- unresolved findings by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

Only after accepted M001 closure should the registry move M002 to `ready`.
