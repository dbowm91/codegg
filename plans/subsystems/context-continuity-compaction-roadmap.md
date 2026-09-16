# Context Continuity and Multi-Compaction Coherence Roadmap

Status: active; M001-M003 closed, M004 ready

Repository baseline reviewed: `db3b69d94fa790bf59a9b0ac093572754eb133af`

Canonical long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Related accepted work:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`
- `plans/implementation/architecture-convergence-incomplete-verticals/001-context-compaction-ownership-convergence.md`
- `plans/closure/architecture-convergence-incomplete-verticals/001-status.md`
- `architecture/compaction.md`
- `architecture/context-compaction-ownership.md`
- `architecture/context-ledger.md`
- `architecture/goal.md`
- `architecture/session.md`

External research basis, inspected before this roadmap:

- OpenAI Codex local checkpoint compaction keeps a generated handoff plus selected original user messages and separately reconstructs initial/world context.
- Newer Codex context management models windows as explicit transitions and relies on durable notes/history references for recovery rather than recursively summarizing every prior window.
- The useful pattern is separation of active prompt state from durable continuation state. CodeGG should not copy the experimental Codex rollover protocol or introduce a second history service; CodeGG already owns durable session/goal state and a restart-durable context artifact store.

Primary Codex source anchors reviewed (research inputs, not CodeGG dependencies):

- `openai/codex@50d77959bf927293c4b5ddcca81d05331ae582ea/codex-rs/core/src/compact.rs`
- `openai/codex@50d77959bf927293c4b5ddcca81d05331ae582ea/codex-rs/core/src/tasks/compact.rs`
- `openai/codex@83dc7d11e873f43533dc898d50fae71cf4d55dc5/codex-rs/core/src/session/token_budget.rs`
- `openai/codex@83dc7d11e873f43533dc898d50fae71cf4d55dc5/codex-rs/core/src/tools/handlers/new_context_window.rs`
- `openai/codex@83dc7d11e873f43533dc898d50fae71cf4d55dc5/codex-rs/ext/history-notes/src/tools.rs`

No ADR is required to begin this roadmap. It extends the already accepted context/compaction ownership and session-storage boundaries. Stop and register an ADR if implementation would move durable session authority out of `codegg-core`, make an external service authoritative for continuation state, or redefine the public session/history contract across frontends.

## 1. Purpose and ownership boundary

Long-running coding work should remain coherent when one logical task crosses many model context windows. The current compactor safely reduces token pressure and preserves tool-call/result invariants, but it still treats the active provider transcript as the main source from which continuation state is reconstructed. That is too lossy for large implementation plans executed over several compactions.

This roadmap makes compaction a transition between bounded **context epochs** over one durable task state.

The target ownership split is:

```text
durable session/goal/todo state (codegg-core)
        |
        +--> ContinuationCheckpointStore
        |      prepared -> installed/aborted
        |      checkpoint lineage + digest
        |
agent runtime assembles authoritative current state
        |
        v
canonical context/compaction owner (src/context)
        |
        +--> deterministic reduction/evidence
        +--> optional semantic enrichment
        +--> bounded replacement history
        `--> checkpoint candidate + diagnostics
        |
        v
AgentLoop sequencing
        |
        +--> persist + verify checkpoint
        +--> install replacement history
        `--> atomically mark installed + durable ContextCompacted event
```

The durable checkpoint is not another memory system and is not a transcript replacement. It is a bounded, typed projection of task state sufficient to resume work, with references back to existing recoverable artifacts where detail is intentionally omitted.

## 2. Current-state findings

The baseline already contains strong foundations:

1. `src/context/compaction.rs` is the canonical production compaction owner. `src/agent/compaction.rs` is a compatibility re-export, so this roadmap must extend the canonical owner rather than add a parallel engine.
2. The hybrid engine deterministically extracts commands, touched files, tests/errors, user constraints, evidence hashes, retained messages, and tool-pair invariants, then optionally asks a model for semantic fields.
3. `GoalStore`, `TodoStore`, `MessageStore`, `EventStore`, and goal revisions are SQLite-backed.
4. Production `context_read` artifacts use `FileArtifactStore` under the workspace `.codegg/context_artifacts` directory and survive daemon restart. No second artifact database is needed.
5. Goal rows already carry objective, plan path, current phase, progress, next action, completion criteria, open questions, and monotonic revision.

The audit also found specific continuity defects:

1. `build_programmatic_state()` creates a `ContextFrame` with deterministic evidence but leaves `user_goal` and `current_task` empty. `semantic_checkpoint()` therefore receives `"unknown"` / `"none"` for those fields and only returns constraints, decisions, unresolved errors, and next steps.
2. `AgentLoop::compact_if_needed()` skips `build_context_frame()` whenever the compacted message set already contains `[codegg compacted session state]`. A hybrid compaction can therefore suppress the later frame that contains `original_user_prompt` and persisted todo state.
3. `compile_programmatic_messages()` and `compile_hybrid_messages()` preserve every existing system message and then append a new compaction frame. A prior CodeGG compaction frame can survive into the next epoch, producing multiple stale or contradictory compacted-state messages over repeated compactions.
4. `ContextLedgerState` records artifact handles, but `to_context_frame()` drops them. Recoverable evidence therefore does not survive into the model-facing frame even though the backing artifacts remain durable.
5. `EvidenceRef` identifiers such as `msg_0001` and `tool_0004` are local to one reduction pass. They are useful diagnostics but are not durable recovery handles.
6. `select_retained_messages()` resolves only the first tool call from a multi-tool assistant message when pulling pair partners. Post-compaction validation catches some bad results, but repeated fallback should not be the ordinary multi-tool path.
7. `ContextCompactedEvent` exists in the durable session-event model, but the production compaction path only publishes in-process `AppEvent::CompactionTriggered`; it does not append the durable event.
8. Goal checkpoint Markdown is append-only, while `read_checkpoint_excerpt()` returns the first N characters. After enough updates the model can see an old prefix while the newest progress/next action lives at the end of the file.
9. `render_goal_context()` and compaction `ContextFrame` are separate projections with different source precedence. The active `Goal` can be authoritative in the initial system prompt while the compactor later falls back to the immutable session-origin prompt.
10. `ResolvedCompactionConfig` says Hybrid is the resolved default, but `compact_context()` enters the hybrid engine only when `compaction.mode` was explicitly configured. The normal production behavior therefore differs from the documented/default resolved policy.

These are correctness and trajectory issues, not requests for a larger context window.

## 3. Invariants

All milestones MUST preserve the following invariants:

- `src/context/compaction.rs` remains the single production owner of context reduction policy.
- `codegg-core` remains the durable owner of session-scoped continuation records.
- `AgentLoop` sequences typed operations; it does not become a second persistence or compaction engine.
- A context rollover never clears environment/workspace/Git state and never grants new authority.
- Original session intent remains immutable provenance. Later user steering is represented separately and cannot silently rewrite historical intent.
- An active durable `Goal` and its revision outrank an inferred transcript summary for long-horizon goal state.
- Deterministically known facts such as touched files, commands, tests, todo/plan state, goal revision, and artifact handles are not replaced by model-generated claims.
- Semantic enrichment may add or refine semantic fields only within host-provided evidence bounds.
- No destructive/replacement compaction is treated as recoverable until its checkpoint is durably persisted and verified.
- Restart ignores prepared/uncommitted checkpoint attempts and only restores an installed checkpoint.
- Repeated compaction produces at most one current CodeGG-owned continuation frame in the provider-visible history.
- Tool-call/result pairing, ordering, and IDs remain valid, including assistant messages with multiple tool calls.
- Existing `ctx://tool/...` handles remain wire-compatible.
- Hidden reasoning, credentials, secret-bearing tool arguments, and unredacted sensitive output are never copied into continuation state.
- Checkpoint payloads, rendered continuation context, and recoverable evidence are explicitly bounded.
- Existing session, goal, todo, artifact, and provider request-context semantics remain readable across migration and restart.
- Verification remains focused and uses existing CI/quick verification; no new CI lane, benchmark gate, or permanent external-model test is introduced.

## 4. Explicit non-goals

This roadmap does not authorize:

- a vector database, semantic-memory service, knowledge graph, or second transcript database;
- copying Codex's private history/notes service or model-only endpoints;
- model-initiated context rollover as a required capability;
- persisting hidden chain-of-thought or provider-private reasoning;
- retaining every historical assistant/tool token indefinitely;
- replacing the existing goal system with continuation checkpoints;
- replacing the existing `FileArtifactStore`;
- changing workspace/project/session identity semantics;
- making full session replay depend on one specific model/provider;
- a generic event-sourcing rewrite;
- widening `context_read` into unrestricted cross-session history access;
- adding broad config knobs before a demonstrated operator need.

## 5. Target continuation model

A durable continuation checkpoint should contain bounded typed state such as:

```text
ContinuationCheckpoint
  identity:
    checkpoint_id
    session_id
    sequence / epoch number
    previous_installed_checkpoint_id
    schema_version
    payload_digest
  provenance:
    session_origin_digest
    active_goal_id + goal_revision (optional)
    plan_path + plan_digest (optional)
    source_history_digest
  intent:
    immutable origin task
    bounded exact recent/user steering spine
  work state:
    current task / phase
    completed + pending + blocked work identifiers
    next action
    open questions
  semantic state:
    constraints
    decisions
    unresolved errors
    next steps
  deterministic evidence:
    touched files
    commands
    tests
    security findings
    bounded recovery references
```

The exact Rust type may differ, but implementation must keep authoritative fields typed and separately identifiable from model-produced semantic fields.

The provider-visible continuation projection is smaller than the durable payload. It must prioritize objective/current work/constraints/decisions/next action and provide exact recovery handles for large evidence rather than embedding the evidence.

## 6. Storage and commit semantics

At baseline the storage layout is version 56. M001 should add the next free sequential migration (v57 if no intervening migration lands) and bump `STORAGE_LAYOUT_VERSION` accordingly.

Use a dedicated additive continuation-checkpoint table rather than reusing:

- the historical `checkpoints` table, whose serialized `Checkpoint` shape owns a different full-session checkpoint contract; or
- `.codegg/goals/*.checkpoint.md`, which is a user-facing append journal.

The store must support at least:

```text
prepare(checkpoint)
load(checkpoint_id)
latest_installed(session_id)
mark_aborted(checkpoint_id, reason)
install_with_compaction_event(checkpoint_id, ContextCompactedEvent)
```

`install_with_compaction_event` must update checkpoint state and append the durable event in one SQLite transaction so restart cannot observe an installed checkpoint without its corresponding lineage event.

Prepared rows are immutable candidates. Sequence gaps caused by abandoned/prepared rows are acceptable; lineage follows explicit `previous_installed_checkpoint_id`, not arithmetic assumptions.

Checkpoint JSON and any human-readable error field must have explicit byte bounds. The stored payload should be hashed with the repository's existing SHA-256 helper or an equivalent deterministic primitive.

## 7. Dependency graph

```text
M001 durable checkpoint + epoch foundation
        |
        +----------------------+
        |                      |
        v                      v
M002 authoritative       M003 bounded exact
intent/plan projection   recovery references
        |                      |
        +----------+-----------+
                   |
                   v
M004 transactional rollover,
multi-compaction/restart qualification,
and production-path activation
```

M002 and M003 may execute in parallel after M001 if separate implementation agents avoid overlapping context-handle/frame code. M004 requires accepted closure of both.

## 8. Ordered milestones

### M001 — Durable continuation checkpoint and epoch foundation

Plan: `plans/implementation/context-continuity-compaction/001-durable-continuation-checkpoint-and-epoch-foundation.md`

Status: **closed** (`plans/closure/context-continuity-compaction/001-status.md`, implementation `fde6c2e3`).

Establish the typed checkpoint identity, additive SQLite storage, prepared/installed/aborted lifecycle, lineage fields, and durable compaction event contract. Wire no model-visible behavior change beyond any required event correctness fix.

Exit condition: CodeGG can durably prepare, verify, install, query, and restart-load a bounded continuation checkpoint, and an installed checkpoint has an atomic durable `ContextCompacted` commit marker.

### M002 — Authoritative intent, plan, and frame projection

Plan: `plans/implementation/context-continuity-compaction/002-authoritative-intent-plan-and-frame-projection.md`

Status: **closed** (`plans/closure/context-continuity-compaction/002-status.md`, implementation `a96ed0fc`).

Build continuation state from host-owned Goal/Todo/runtime evidence plus bounded exact user intent, fix source precedence, fix stale goal-journal rendering, eliminate stacked CodeGG compaction frames, and make semantic enrichment update only fields it owns.

Exit condition: one compacted continuation frame contains the correct objective/current task/constraints/decisions/next action across repeated reductions and cannot be displaced by an older frame or an `"unknown"` goal.

### M003 — Bounded exact context recovery references

Plan: `plans/implementation/context-continuity-compaction/003-bounded-exact-context-recovery-references.md`

Status: **closed** (`plans/closure/context-continuity-compaction/003-status.md`, implementation `3ea77e9f`).

Extend the existing context-artifact/handle surface so a checkpoint can point to bounded exact evidence discarded from the active prompt. Preserve `ctx://tool/...`, use the existing durable `FileArtifactStore`, and do not expose hidden reasoning or sensitive tool inputs.

Exit condition: model-visible continuation state can recover important omitted evidence by exact same-session handles after restart without adding a new history database or unbounded search tool.

### M004 — Transactional rollover and multi-compaction qualification

Plan: `plans/implementation/context-continuity-compaction/004-transactional-rollover-and-multi-compaction-qualification.md`

Status: **ready** (M002 accepted closure at `plans/closure/context-continuity-compaction/002-status.md`; M003 accepted closure at `plans/closure/context-continuity-compaction/003-status.md`).

Integrate checkpoint preparation/verification with `compact_if_needed()`, load installed continuation state on later turns/restart, reconcile the actual hybrid/default path, and prove trajectory stability through many forced compactions and crash boundaries.

Exit condition: a synthetic long-running task with large plan state, later user steering, tool activity, and restart interruptions can cross at least eight forced compactions without losing or reverting the authoritative objective, active work, decisions, next action, test/file state, or resolvable evidence.

## 9. Failure, cancellation, restart, and concurrency policy

Checkpoint generation and installation are host-controlled.

- Cancellation before replacement leaves the current history unchanged and any prepared checkpoint uninstalled/aborted.
- Persistence or read-back verification failure before replacement must not silently install a lossy history. At ordinary threshold pressure, defer the rollover and report diagnostics.
- If the provider would otherwise exceed hard capacity, the existing pair-safe emergency compaction may still be used as a bounded availability fallback, but it must report degraded continuity and must not fabricate an installed durable epoch.
- Semantic model failure falls back to host-only continuation state.
- Concurrent checkpoint attempts for one session serialize through the SQLite transaction/lineage precondition. A stale parent checkpoint cannot install over a newer installed checkpoint.
- Restart loads only `Installed` checkpoints whose payload digest and schema version validate. `Prepared` rows are diagnostic/reclaimable state, never resume authority.
- Missing optional evidence handles degrade to summaries/diagnostics; they do not invalidate the entire checkpoint. Missing authoritative goal/session rows must be classified explicitly.
- A corrupt installed checkpoint fails closed to existing session/goal state and emits a visible diagnostic; it is never silently parsed partially.

## 10. Security and retention

Continuation state contains model-visible task context and therefore inherits session-content sensitivity.

Implementation must:

- reuse existing redaction/projection paths for tool evidence;
- never persist `PartData::Reasoning` or hidden provider reasoning into continuation artifacts;
- never persist raw authorization headers, credentials, environment secrets, or secret-bearing tool arguments;
- retain same-session enforcement for recovery handles;
- keep artifact and checkpoint payload sizes bounded;
- preserve current export/redaction rules; continuation records should either be excluded from exports initially or receive an explicit bounded/redacted export contract before inclusion;
- avoid logging checkpoint bodies; logs/events carry IDs, digests, sizes, and state transitions.

## 11. Observability

Use existing diagnostics and session events. Add bounded fields sufficient to answer:

- which checkpoint/epoch is current;
- which prior checkpoint it superseded;
- tokens before/after;
- checkpoint payload size/digest;
- deterministic vs semantic enrichment path;
- fallback/degraded-continuity reason;
- number of retained exact intent items and recovery refs.

Do not log full checkpoint bodies or artifact contents.

## 12. Verification posture

Each implementation plan defines narrow tests. Broad verification remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

The final milestone additionally owns deterministic trajectory/restart integration tests. It must not add public-network/model tests, a new CI matrix, or benchmark thresholds.

## 13. User-visible completion criteria

This roadmap is complete when:

- long-running tasks no longer depend on recursive summaries as the sole continuity mechanism;
- active goal/plan/todo state survives repeated context reduction with explicit source precedence;
- later user steering remains represented after earlier compactions;
- only one current CodeGG continuation frame appears after a rollover;
- omitted detail remains recoverable by bounded exact handles where retained;
- daemon restart can restore the last installed continuation checkpoint without trusting prepared partial state;
- compaction persistence failure cannot silently destroy the only copy of required task state;
- repeated multi-tool turns still satisfy call/result invariants;
- ordinary short sessions retain current behavior and no new memory/history service is introduced.

## 14. Risks and deferred work

Primary risks are schema churn, accidentally making a model summary authoritative, checkpoint bodies growing without bound, and widening read access through generalized context handles. The milestone plans constrain each of these.

Deferred unless new evidence justifies them:

- semantic history search across the entire session;
- vector retrieval over prior windows;
- cross-session continuation or shared team memory;
- remote/coordinator replication of context artifacts beyond existing workspace/node ownership;
- user-facing context-epoch management commands;
- provider-specific compaction endpoints;
- automatic garbage collection policy for old checkpoint/evidence artifacts beyond a simple bounded retention rule justified during implementation.
