# Project Work Orders and Task View M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view/002-release-coordinator-session-materialization.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Repository baseline reviewed: `98688fa6`

Implementation commits or pull requests:

- `c54656e5` — work-orders M002: release coordinator, session
  materialization, workspace isolation (gate evaluator/store
  primitives/daemon coordinator/AgentTurn scheduler admission/
  worktree isolation/model-policy narrowing/events/guard/docs/tests)

## 1. Executive finding

M002 is complete. Ready WorkOrder occurrences materialize exactly once
into ordinary canonical sessions through the existing global scheduler:
one occurrence creates at most one session and one initial `AgentTurn`
submission; duplicate wakes, concurrent claims, and restart at every
partial-materialization boundary converge instead of duplicating;
immediate/delay/not-before/sequence/finite-repeat gates survive restart;
concurrent Git mutation tasks receive distinct managed worktrees;
non-Git mutation never claims isolation; removed models and tightened
policy produce attention/narrowing rather than silent fallback/widening;
sequence failure holds downstream work by default. No second scheduler,
no direct `AgentLoop` construction from the coordinator, no speculative
sessions, no ad-hoc directory copying. This is capability/invariant work
as the plan classifies it; the new TUI (M003) is explicitly out of scope
and remains blocked until this closure.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Release-gate evaluator + persisted latching (§6, §13A) | `coordinator.rs::evaluate_occurrence_gates`; `store::persist_gate_evaluation`; `merge_latches` | pass | Immediate/delay/not-before/sequence-ready/external-trigger + All/Any; backward-clock latch; delay deadline persisted in `not_before` so restart reuses the origin |
| `next_check_at`/timer wake, indexed/bounded due set (§5, §13A) | `store::list_due_occurrences[_global]`; `coordinator::evaluate_due_for_project` (cap 128/project, 512 global) | pass | Coordinator reads only rows whose `next_check_at` is absent/due; never polls every work order |
| Sequence-ready evaluation (§6, §13A) | `coordinator::sequence_ready_for` + `sequence_holds`/`sequence_predecessors_terminal` | pass | Lane order + latest-predecessor terminal state; HoldLane default holds failed/cancelled/needs-attention |
| Finite-repeat occurrence creation (§11, §13A) | `store::create_next_repeat_occurrence` → `RepeatOutcome{duplicate}` | pass | Only latest terminal spawns successor; duplicate wakes converge; distinct ids; budget enforced |
| Exactly-once claim/materialization state machine (§7, §13B) | `store::claim_occurrence` CAS; `persist_workspace/session/job_link` idempotent; `transition_occurrence`; `PartialStage` | pass | `waiting → ready → claiming → running → terminal/needs_attention`; links persist before next side effect |
| Project/workspace/repository resolution (§8) | `CoreDaemon::resolve_shared_workspace`/`project_has_git_repository` via `workspace_project_binding` + `WorkspaceRegistry` + `find_git_root` | pass | No ad-hoc directories; missing binding fails closed to attention |
| AutoIsolated/shared-safe policy (§8, §13C) | `coordinator::resolve_workspace_action` → `WorkspaceAction` | pass | Git mutation → managed worktree; read-only → share; serialized/shared mutation → scheduler exclusivity; non-Git mutation → attention (`isolation_unavailable`) |
| Managed worktree acquisition (§8, §13C) | `CoreDaemon::allocate_managed_worktree` via `WorktreeService::create` | pass | Lazy at claim/start; recovery reuses stored link; dirty/conflict retention stays in worktree service |
| Canonical session creation + origin attribution (§10, §13D) | `CoreDaemon::create_canonical_session` via `SessionStore::create_with_id` deterministic id | pass | Ordinary session; linkage column (never title-encoded); title stays bounded label |
| Model/provider/approval/sandbox re-resolution (§9, §13D) | `resolve_model`; `narrow_approval`/`narrow_sandbox`; provider-kind check at claim | pass | Removed → `model_unavailable`, no fallback; snapshots narrow, never widen; per-turn ceilings still enforced on `TurnSubmit` path |
| Initial AgentTurn via scheduler boundary (§10, §13D) | `JobSubmissionService` AgentTurn validation + `AgentTurnExecutor` + `submit_initial_turn` deterministic key + `reconcile_by_key` | pass | `JobPayload::AgentTurn.submission_key` additive (`None` for legacy); scheduler remains sole admission authority |
| Occurrence→session/job/workspace/worktree linkage (§10) | `session_id`/`job_id`/`workspace_id`/`worktree_id` columns + `occurrence_for_session/job` | pass | Nullable until materialization; monotonic/idempotent after claim |
| Cancellation propagation + sequence hold (§11, §12, §13E) | `cancel_occurrence`/`cancel_work_order_occurrence` (job-store cancel first, then terminal); `finish_occurrence` + repeat/repeat-wake | pass | Pre-claim creates nothing; running routes canonical cancel; terminal cancel idempotent; completion-vs-cancel deterministic (terminal wins) |
| Restart reconciliation (§7, §11, §13E) | `list_incomplete_occurrences`; link-before-next-side-effect ordering; `reconcile_work_orders_for_project` | pass | Fault-injection tests crash after claim/workspace/session/job-submit and converge |
| Structural events/diagnostics + docs (§13F) | `CoreEvent::WorkOrderOccurrenceChanged` (Safe) + `work_order_lifecycle` audit reuse; `architecture/work_orders.md` + `scheduler.md` | pass | Identity/state only; no bodies/secrets/reasoning |
| Fault-injection seams (§13B, §14) | `MaterializationFault` + stage-boundary tests | pass | Crash/reopen after claim, worktree create, session create, job submit |
| Static ownership guard (§13F) | `scripts/check_work_order_coordinator.py` + `docs/execution-ownership.toml` entry | pass | Forbids AgentLoop/scheduler/bypass/copy/dir-copy in coordinator; forbids execution refs in core module |

## 3. Production implementation evidence

Ownership landed:

- `crates/codegg-core/src/work_order/coordinator.rs` (new): pure
  gate evaluation (`GateEvaluation`, `delay_deadline_for_occurrence`,
  All/Any join, latched-gate clock-backward immunity), sequence
  (`sequence_holds`, `sequence_predecessors_terminal`),
  repeat (`is_repeat_exhausted`, `next_occurrence_index`), deterministic
  keys (`session_id_for_occurrence` → `wo-session-<occurrence>`,
  `submission_key_for_occurrence` → `wo-turn-<occurrence>`, sanitized +
  bounded to `MAX_IDEMPOTENCY_KEY_LEN`), workspace policy
  (`WorkspaceAction`), model/policy narrowing. No session/job/worktree/
  provider construction (guard-enforced).
- `crates/codegg-core/src/work_order/store.rs` (extended): due-set
  queries, `persist_gate_evaluation`, `latch_external_trigger` (internal
  M005 seam), `claim_occurrence` CAS, idempotent
  workspace/session/job link persistence with mismatch conflicts,
  matrix-validated `transition_occurrence`, precedence-safe
  `cancel_occurrence`, exactly-once `create_next_repeat_occurrence`,
  `list_incomplete_occurrences`, `occurrence_for_session/job`.
  No schema migration: all M002 state fits M001 columns
  (`gate_latches_json`, `not_before`/`next_check_at`, linkage columns,
  `attention_code`/`diagnostic`, timestamps). `STORAGE_LAYOUT_VERSION`
  unchanged.
- `crates/codegg-core/src/work_order/model.rs`: `GateKind` gains
  `PartialOrd`/`Ord` for deterministic latch ordering (wire-compatible).
- `crates/codegg-core/src/jobs/mod.rs`: `JobPayload::AgentTurn` gains
  additive `submission_key: Option<String>` (`#[serde(default,
  skip_serializing_if)]`; legacy payloads decode as `None`).
- `crates/codegg-protocol/src/core.rs`: additive
  `CoreEvent::WorkOrderOccurrenceChanged{project_id, work_order_id,
  occurrence_id, change, state}`; `PROTOCOL_VERSION` stays 2.
- `crates/codegg-core/src/projection_replay/safe_publication.rs`: new
  event classified `Safe` (identity/state only).
- `src/scheduler/submission.rs`: `validate_payload` admits
  `(AgentTurn, AgentTurn{prompt, agent})` (both non-empty);
  `payload_matches_submission_key` matches AgentTurn `submission_key`
  for post-restart reconciliation.
- `src/scheduler/executor.rs`: `ExecutorKind::AgentTurn` (`agent_turn`);
  `executor_kind_for_job` routes `JobKind::AgentTurn` to it.
- `src/scheduler/executors.rs`: scheduler-owned `AgentTurnExecutor`
  (validates payload + runtime, holds the admission permit, completes
  with "initial turn admitted" summary; never constructs `AgentLoop`).
  Registered in `register_default_executors`.
- `src/scheduler/scheduler.rs`: default-executor registration installs
  `AgentTurnExecutor`.
- `src/core/work_order_coordinator.rs` (new, `scheduler` owner in
  `docs/execution-ownership.toml`): `WorkOrderCoordinator`
  (due evaluation, lane readiness, partial-stage description, policy
  helpers) plus daemon production wiring (`wake_work_orders_for_project`,
  `materialize_ready_occurrence`, `allocate_managed_worktree`,
  `create_canonical_session`, `submit_initial_turn`,
  `reconcile_work_orders_for_project`,
  `cancel_work_order_occurrence`, `finish_occurrence`,
  `publish_occurrence_changed`). Every initial turn enters
  `JobSubmissionService::submit`; scheduler wake uses existing
  `WokeReason::JobEnqueued`.
- `src/core/mod.rs`: registers `work_order_coordinator`.
- `src/job_dispatcher.rs`, `crates/codegg-core/src/jobs/mod.rs` tests:
  updated `AgentTurn` literals for the additive field.
- `scripts/check_work_order_coordinator.py` (new): ownership guard
  (comment-aware).
- `docs/execution-ownership.toml`: coordinator site entry.
- `architecture/work_orders.md`: M002 coordinator/materialization
  section + test commands; `architecture/scheduler.md`: AgentTurn
  executor paragraph.
- Tests: `coordinator.rs` unit (11), `store.rs` M002 (5),
  `src/core/work_order_coordinator.rs` (3),
  `tests/work_orders_m002_materialization.rs` (10, incl. real
  `JobSubmissionService` admission, real `SessionStore` canonical
  creation, real `WorktreeService` dual-worktree isolation, fault
  injection at all four boundaries).

Deliberate adjustments from the plan text (semantics preserved):

- No storage migration: the M001 columns already carry every M002
  durable field, so no `migrate_v62`/layout bump was needed. Delay
  deadlines persist in the existing `not_before` column (set on first
  evaluation alongside `next_check_at`).
- New attention needs (`workspace_unavailable`,
  `isolation_unavailable`, scheduler failures) map onto the existing
  closed `AttentionCode` set (`MaterializationFailed`/
  `WorktreeConflict`/`ModelUnavailable`/predecessor codes) with
  `isolation_unavailable:`/`workspace_unavailable:`-prefixed bounded
  diagnostics, avoiding a CHECK-constraint migration while staying
  truthful (isolation is never mislabeled).
- `AgentTurnExecutor` records scheduler admission ("initial turn
  admitted; session drivable via TurnSubmit") rather than driving an
  LLM turn inline: the coordinator must not construct `AgentLoop`
  (stop condition), and no `AgentTurn` executor existed. The occurrence
  stays `Running` until canonical session/job terminal state is
  projected back — job completion alone never completes an occurrence.
- Approval/sandbox ceilings at claim time are `None` (no extra ceiling
  beyond the snapshot) because per-turn ceilings are enforced by the
  existing `TurnSubmit` preference path; the coordinator additionally
  enforces provider availability + snapshot narrowing, so later
  tightening still narrows/blocks at turn time and snapshots never
  widen.
- `WokeReason::JobEnqueued` (existing) wakes the scheduler after
  materialization; no new wake-reason variant was added.

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test -p codegg-core --lib -- work_order
cargo test -p codegg-protocol work_order
cargo test -p codegg --lib -- work_order
cargo test -p codegg --lib -- scheduler
cargo test -p codegg-core --lib -- worktree_service
cargo test -p codegg-core --lib -- worktree
cargo test --test worktree
cargo test --test reliability_qualification_m008
cargo test --test work_orders_m002_materialization
cargo test --test work_orders_m001_foundation
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
python3 scripts/check_work_order_coordinator.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

(`cargo test -p codegg-core -- work_order` from the plan matches no
lib target by that filter form (0 tests); the substitute is
`cargo test -p codegg-core --lib -- work_order`, recorded here
explicitly. `cargo test --test execution_reliability` has no equivalent
target name; the substitute is the execution-reliability qualification
suite `reliability_qualification_m008`, recorded here explicitly.)

### Results (local)

- `codegg-core --lib work_order`: 35 passed (19 M001 + 11 coordinator
  policy + 5 M002 store primitives).
- `codegg-protocol work_order`: 6 passed.
- `codegg --lib work_order`: 3 passed (due-set/sequence coordinators).
- `codegg --lib scheduler`: 78 passed (incl. AgentTurn submission
  paths; no executor-kind regression).
- `codegg-core --lib worktree_service` / `worktree`: 9 / 12 passed.
- `worktree` integration: 14 passed.
- `reliability_qualification_m008`: 39 passed.
- `work_orders_m002_materialization`: 10 passed in ~1 s (one
  occurrence → one session → one AgentTurn job; concurrent-claim
  single winner; crash recovery at all four boundaries; canonical
  session key; dual managed-worktree isolation on a real temp git
  repo; truthful non-Git/shared policy; model/policy narrowing;
  cancellation at every boundary + terminal-wins race; delay/
  not-before/sequence/repeat restart survival; duplicate-wake
  session/job convergence).
- `work_orders_m001_foundation`: 12 passed (no M001 regression).
- Guards: execution-ownership ok, authorization matrix verified,
  core-boundary pass, work-order coordinator ownership ok, fmt clean,
  clippy (`--workspace --all-targets --locked -- -D warnings`) clean,
  `verify.sh quick` pass.

## 5. Invariant review

| Plan §3 invariant | Evidence |
|---|---|
| Coordinator, not scheduler/executor | `WorkOrderCoordinator` evaluates/claims/links only; admission + execution stay in scheduler/executors; guard forbids `JobScheduler::new`/`spawn_loop`/`AgentLoop::new` in coordinator |
| Every initial turn enters JobSubmissionService + scheduler | `submit_initial_turn` → `JobSubmissionService::submit` (deterministic key) + `WokeReason::JobEnqueued`; `AgentTurnExecutor` registered in default set; integration asserts 1 durable `AgentTurn` job |
| One occurrence → at most one session + one submission | Deterministic `wo-session-*`/`wo-turn-*` keys; link-before-next-side-effect; mismatch conflicts; duplicate-wake + concurrent-submit convergence tests |
| Restart/duplicate wake cannot duplicate | CAS claim (exactly-one-winner tests); idempotent links; `reconcile_by_key`; crash-after-each-boundary recovery tests |
| Running sessions are ordinary sessions | `create_canonical_session` via `SessionStore::create_with_id`; steering/permission/question/cancel/WorkPlan stay on existing paths (no WorkOrder-specific runtime) |
| Requests revalidated, never widened | Provider-kind availability check at claim; `narrow_*` min(snapshot, ceiling); removed → attention; per-turn ceilings still enforced on `TurnSubmit` |
| Lazy worktree allocation | `allocate_managed_worktree` at claim/start only; authoring paths allocate nothing; recovery reuses stored link |
| Non-Git mutation not isolated | `resolve_workspace_action` returns `NeedsAttention(isolation_unavailable)`; test asserts diagnostic; no directory copying anywhere (guard) |
| Failed/cancelled/attention predecessors hold | `sequence_holds` HoldLane default + `sequence_ready_for`; success-advances/failure-holds tests |

## 6. Failure and recovery review

- Duplicate delivery: claim CAS admits one winner (`StateConflict`
  for losers); external-trigger latch idempotent; submission/batch keys
  converge; repeat successors converge (`duplicate: true`).
- Stale writers: lane/work-order CAS unchanged from M001; occurrence
  linkage mismatches conflict explicitly with zero overwrite.
- Concurrent claims/submissions: exactly-one-winner tests for claim,
  session convergence, and job submission (same deterministic key →
  one durable job).
- Restart: file-equivalent reopen (new service over same pool)
  preserves claim/links/deadlines; `list_incomplete_occurrences`
  surfaces partial rows; recovery queries canonical session/job stores
  before creating; delay deadlines reuse persisted `not_before`.
- Partial persistence: each boundary persists its link before the next
  side effect; fault injection crashes after claim, workspace link,
  session link, and job submit, then converges.
- Malformed/unauthorized input: unknown trigger refs rejected;
  cross-project scope mismatches fail closed via project-scoped store
  methods; terminal-cancel idempotent.
- Cancellation: pre-claim (nothing created), mid-materialization
  (session linked, no new job submitted), running (job-store cancel
  first, then terminal), terminal (idempotent); completion-vs-cancel
  race resolves to recorded terminal.
- Sequence: failed/cancelled/needs-attention predecessors hold;
  held sequence stays held after predecessor terminal failure until
  explicit resolution (no auto-advance).

## 7. Migration and compatibility review

- No migration: M002 uses M001 columns only; `STORAGE_LAYOUT_VERSION`
  unchanged; pre-M002 databases gain behavior with empty/new rows only.
- Protocol additive: one `CoreEvent` variant
  (`WorkOrderOccurrenceChanged`) + one additive optional payload field
  (`AgentTurn.submission_key`); `PROTOCOL_VERSION` stays 2; legacy
  `AgentTurn` payloads decode (`None` key, reconciled via occurrence
  linkage); old clients ignore the unknown event variant.
- Scheduler compatibility: `ExecutorKind::AgentTurn` is additive;
  existing executor registrations untouched; `AgentTurn` jobs that
  predate the executor would previously have been unschedulable and
  now admit (documented behavior change, forward-only).
- Rollback: downgrading below this change leaves occurrence linkage
  rows inert but present; new event variant is ignored by older
  readers; no data migration to reverse.

## 8. Security review

- Authorization: coordinator reuses project-scoped `WorkOrderService`
  methods (server-side project resolution preserved from M001); no new
  principal surface; revoked authority fails closed to attention
  before new execution (no retained hidden capability).
- Privacy: occurrence events carry identity/state only; session/job
  lookups stay project-scoped; denials keep the M001 not-found shape.
- Secrets: no trigger secret material in M002 (latch field only; M005
  owns verifiers); DTO/event JSON carry no prompt bodies, credentials,
  or reasoning; submission keys are opaque retry identities, not
  credentials.
- Attribution/audit: actor mutations keep structural
  `work_order_lifecycle` audit; daemon-initiated coordinator
  transitions emit structural events (no forged actor attribution).
- Bounds as DoS control: due-set caps (128/project, 512 global,
  64 reconcile), bounded diagnostics, deterministic key lengths,
  lane/member/repeat/gate bounds unchanged from M001.
- Isolation truthfulness: Git mutation uses the managed-worktree
  lifecycle (leases, retention, dirty/conflict attention owned by
  `WorktreeService`); non-Git mutation is never labeled isolated.

## 9. Documentation and operations

- `architecture/work_orders.md`: M002 coordinator/materialization
  section (state machine, gates, idempotency keys, workspace/model/
  policy rules, cancellation, events) + test commands.
- `architecture/scheduler.md`: `AgentTurnExecutor` paragraph +
  executor-kind list.
- `docs/execution-ownership.toml`: coordinator site entry
  (`scheduler` owner, `JobSubmissionService::submit` entrypoint).
- `scripts/check_work_order_coordinator.py`: ownership guard (run in
  closure; not a new CI lane per verification policy).
- Operator diagnostics: occurrence `change` hints (`claimed`,
  `running`, `attention`, `completed`, `failed`, `cancelled`,
  `repeat`); attention diagnostics prefixed (`model_unavailable:`,
  `isolation_unavailable:`, `workspace_unavailable:`,
  `worktree_conflict:`, `materialization_failed:`,
  `scheduler submission failed:`); capability negotiation unchanged
  (M001 bounds).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `AgentTurnExecutor` records admission; it does not drive an LLM turn inline (coordinator must not construct `AgentLoop` per stop conditions) | Initial turn admission is durable and scheduler-owned, but first agent prose still starts via the ordinary `TurnSubmit` path on attach | M003/M007 to define first-turn auto-drive UX; no corrective pass: documented semantics, occurrence stays `Running` until canonical terminal |
| low | Claim-time approval/sandbox ceilings are `None` (snapshot stands); per-turn ceilings enforced on the existing `TurnSubmit` path | Policy tightening between claim and first turn narrows/blocks at turn time rather than at claim time | Future milestone may thread project/admin ceilings into the claim step; current behavior never widens (min at turn time) |
| low | Daemon wake/reconcile entry points (`wake_work_orders_for_project`, `reconcile_work_orders_for_project`) are wired but not yet driven by a timer/predecessor-event hook | Due work materializes on explicit wake (tests + API), not autonomously on a loop | M003 (or a narrow daemon hook) to connect timer/`next_check_at`/predecessor wakes; coordinator never polls on the scheduler tick by design |
| low | Lane member removal has no protocol operation (carried from M001) | Cancelled members occupy positions until sequence evaluation skips them | M002/M003 to decide detach semantics if needed (unchanged from M001) |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed: M002 exit
condition is satisfied (immediate/time/delay/sequence WorkOrders
materialize exactly once into normal sessions/jobs; concurrent Git
mutation tasks receive distinct managed worktrees; failures hold
sequence by default).

Blocked-work audit (registry `Blocked work` + roadmap §6 dependency
graph):

- M003 (project Task composer/view, hard dep M002) → **ready**.
  Its sole hard dependency (M002 closure) is now satisfied.
- M004 (global Workspace dashboard, hard dep M003) → remains
  **blocked** on M003 closure.
- M005 (external trigger endpoint, hard dep M002; scheduled after M004
  for handoff clarity) → remains **blocked**: hard dependency
  satisfied, but registry sequencing keeps one clear handoff chain
  after M004. Blocker note updated accordingly.
- M006 (agent WorkOrder tool, hard dep M002/M001 service; scheduled
  after M005) → remains **blocked**: hard dependencies satisfied,
  but ordered handoff clarity keeps it after M005. Blocker note
  updated accordingly.
- M007 (qualification, hard dep M001-M006) → remains **blocked** on
  M001-M006 closure.

## 12. Registry updates

- Move M002 from dependency-ready to closed with this closure record
  and implementation `c54656e5`.
- Move M003
  (`003-project-task-composer-and-view.md`) from blocked to ready: its
  hard dependency (M002 closure) is now satisfied.
- Keep M004 blocked on M003 closure.
- Keep M005 blocked (hard M002 dependency satisfied; held for ordered
  handoff after M004).
- Keep M006 blocked (hard M002/M001 dependencies satisfied; held for
  ordered handoff after M005).
- Keep M007 blocked on M001-M006 closure.
- Record M002 under recently closed work.
- Update the subsystem roadmap M002 status to closed and M003 to ready.
