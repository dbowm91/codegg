# Project Work Orders and Task View M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view/007-work-order-trajectory-and-recovery-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Repository baseline reviewed: `3ed785618bfac5a85f504813bdb7fc3a923e8679`
(M007 plan baseline; qualification developed on the M006-closure tree)

Implementation commits or pull requests:

- `0216e814` — work-orders M007: trajectory/recovery qualification harness
  (new `tests/work_orders_m007_trajectory.rs`, 14 deterministic
  integration tests covering qualification matrix A-L plus the
  representative 15-plan trajectory and the §9 ownership audit; M007
  architecture section in `architecture/work_orders.md`) — harness-only
  qualification, no production delta.

## 1. Executive finding

M007 is complete. The composed M001-M006 capability was qualified under
restart, duplicate delivery, concurrent team mutation, policy/model
drift, worktree conflict, cancellation, permission wait, sequential
failure, external-trigger replay, and agent-created batch trajectories:
exactly-once occurrence materialization survives all eight fault
windows with one canonical session and one initial `AgentTurn` job;
sequence-lane order/reorder/failure-hold is CAS-safe across restart and
concurrent principals; gate/repeat/trigger semantics are deterministic,
finite, and replay-safe; parallel Git mutation tasks receive distinct
managed worktrees while non-Git isolation stays truthful; model/provider
drift and policy narrowing produce explicit attention, never silent
substitution or widening; permission/steering/cancellation use ordinary
session mechanisms after materialization; trigger capabilities stay
narrow, verifier-only, idempotent, revocable, and secret-clean;
agent batch creation stays atomic/idempotent/bounded; global/project
projections stay bounded, project-correct, privacy-preserving, and
stale-completion resistant; the representative 15-plan trajectory
reaches its designed states in order through injected failures and
restart; and exactly one scheduler/admission owner exists. No
corrective production change was required: qualification found no
defect above low severity, so per the plan's corrective-change policy
(§10) this milestone lands tests + docs only. This is
invariant/capability closure as the plan classifies it; the Project
Work Orders and Task View workstream is fully closed by this record.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| A: default immediate create -> claim once -> one session -> one AgentTurn -> running Task row; crash after each durable step converges (§6A) | `a_creation_immediate_materializes_once_with_restart_convergence` | pass | Duplicate claim conflicts; workspace/session/job links persist in production order; reopen reconciles by key; no fake Session rows for waiting work |
| B: 15-member lane order/revision stable; future-only reorder; pinned running head; stale CAS zero-mutation; success releases next only; failure/cancel/attention holds; explicit retry; no mass release (§6B) | `b_fifteen_lane_order_restart_reorder_hold` | pass | Claimed head reorder rejected `StateConflict`; Failed/NeedsAttention hold under `HoldLane`; retry is attention -> waiting (Failed is terminal by the matrix) |
| C: per-gate release; All/Any joins; per-occurrence latch; duplicate-event harmlessness; explicit stable timestamps; restart before/after; invalid configs rejected (§6C) | `c_release_gate_join_latch_duplicate_timezone_restart` | pass | 2030-01-01T00:00:00Z == 1893456000000 ms round-trips exactly; immediate+delay combination rejected |
| D: exact counts 1/2/max, invalid over-bound; distinct identities; restart between occurrences; delay anchoring; cancel remaining; no unbounded recurrence (§6D) | `d_finite_repeat_exact_counts_exhaustion` | pass | 256 accepted, 257 rejected; repeat session/job keys differ; duplicate wake converges (`duplicate: true`) |
| E: fault injection at all 8 windows; forward convergence on durable identities; never a second session/job; no metadata-repair side-effect replay (§6E) | `e_fault_windows_converge_without_duplicates` | pass | Windows: pre-claim, post-claim, post-workspace-link, post-reservation, post-session-create, post-session-link, post-job-submit, post-ack + shutdown-while-running; exactly 1 job row |
| F: 3 concurrent Git mutations get distinct worktrees/leases/paths/provenance; clean/dirty/conflicted/cancel/restart/cleanup; non-Git truthfulness (§6F) | `f_worktree_isolation_and_truthfulness` | pass | Only `Clean` is cleanup-safe; non-Git mutation is `isolation_unavailable` attention or `ShareSerialized`, never isolated |
| G: removed model, disabled provider, narrowed approval/sandbox, revoked creator capability (§6G) | `g_model_provider_policy_drift` | pass | Empty catalog resolves requested models to attention; post-claim payload edits are `StateConflict`; creator attribution immutable |
| H: permission wait with dashboard signal; authorized answer vs unauthorized deny; ordinary steering; cancel propagation; downstream hold (§6H) | `h_permission_steering_cancel_use_ordinary_session_mechanisms` | pass | Sync `PermissionRegistry` scoped to the materialized ordinary session; cross-session answer rejected; session row independently readable |
| I: narrow fire; method/body/query hygiene; verifier-only storage; privacy-safe failures; duplicate/concurrent idempotency; expiry/revoke/max-fire; bearer isolation; secret-free logs (§6I) | `i_trigger_replay_security_matrix` | pass | Concurrent distinct keys cannot double-release; max-fire/expiry/revoke fail closed; `Debug` shapes carry no bearer |
| J: plan-file queue via ordinary file tools + one batch; first-immediate + sequential rest; exact retry dedupe; invalid item N aborts fully; authority/fan-out/depth bounds; restart durability (§6J) | `j_agent_batch_idempotency_bounds` + M006 suite (21 green) | pass | Agent 16 <= human 32 batch, agent repeat 8 <= human 256, depth 4 pinned in-test; invalid batch leaves row count unchanged; cross-project lane rejected |
| K: concurrent reorder race; edit-vs-claim; cancel-vs-claim/completion; viewer scoping; not-found privacy; dashboard counts; observer permission/steer denial (§6K) | `k_team_contention_and_privacy` | pass | Same-revision concurrent reorders admit exactly one winner; foreign-project id lookups return None; cancel-after-terminal idempotent |
| L: rapid switching with in-flight snapshots; epoch/generation guards; Vim navigation project-correctness (§6L) | `l_tui_stale_completion_and_navigation` | pass | Stale dashboard generations/expansions drop; `moved_lane_order` never moves the pinned head; project switch replaces identity |
| Representative 15-plan trajectory (§7) | `trajectory_fifteen_plan_end_to_end` | pass | Immediate + delayed + NotBefore/sequence + trigger-gated + repeat + 2 parallel + permission wait + failure/attention retry + restart-while-running + restart-with-future + agent batch segment; distinct session/job identities, no duplicates |
| Observability/audit review (§8) | Bounded structural events/audit from M001/M002/M004/M005 retained; secret-free assertions in I/J | pass | No high-cardinality prompt content or secret values in labels (trigger `Debug`/metadata censuses) |
| Static ownership/duplication audit (§9) | `ownership_single_scheduler_and_boundaries` + guards below | pass | One scheduler owner; one core persistence owner; typed non-path identities; `task` vs `work_order` separation |
| Corrective-change policy (§10) | No defect above low found; no production delta | pass | No ADR/domain-ownership invalidation; no masked predecessor incompleteness |
| Documentation reconciliation (§12) | `architecture/work_orders.md` M007 section; cross-doc audit | pass | jobs/scheduler/session/worktree/authorization/tui/protocol/README claims confirmed consistent; no contradictions |

## 3. Production implementation evidence

No production delta. M007 is harness-only qualification (same posture
as long-horizon M005, closed harness-only at `35d8a955`): every
invariant under test is owned by already-closed milestones
(M001 domain/store/protocol `a856f2e0`, M002 coordinator/materialization
`c54656e5`, M003 composer/view `79a00460`, M004 dashboard `8c6e8190`,
M005 trigger `f22c9d8d`, M006 agent tool `38e533a8`). Qualification
exercised the actual landed implementations rather than the plan text;
no material deviation from the M001-M006 plans was found during this
pass (the only adjustments are test-level encodings documented below,
none of which change production semantics).

Landed:

- `tests/work_orders_m007_trajectory.rs` (new, 14 integration tests,
  ~1650 lines): matrix A-L + §7 trajectory + §9 ownership probes,
  reusing deterministic in-memory/file-DB/SQLite fixtures,
  `JobSubmissionService` + `InMemoryJobStore` scheduler harness,
  canonical `SessionStore`, managed `WorktreeService::memory`, sync
  permission registry, and pure TUI state transitions. No live
  providers, no network, no second scheduler, no new harness framework.
- `architecture/work_orders.md`: M007 section (per-matrix evidence
  summary, ownership pins, test commands, re-verified suite counts).

Deliberate test-level encodings (semantics preserved):

- Failed occurrences are terminal by the closed M001 matrix
  (`can_transition_occurrence` admits no out-transition from
  `Failed`), so "explicit retry" is exercised as the documented
  attention -> waiting resume (Running/Waiting -> NeedsAttention ->
  Waiting), with the Failed-holds predicate pinned separately. The
  plan's "retry/skip/cancel/advance" all resolve through these two
  documented paths.
- Trigger duplicate-`Idempotency-Key` delivery converges on the stored
  receipt, so a replay returns the same `receipt_id` with
  `duplicate: true` and the latched value of the original delivery
  (stable semantics), rather than a fresh `latched: false`. Both the
  I-matrix test and the trajectory assert this converged shape.
- The trajectory's permission-wait (step 7) and failure/attention
  (step 8) members carry immediate gates with lane membership (as
  produced by the real Task composer default), so they materialize
  with the releasable spine and then cycle through attention ->
  waiting; strict sequence gating of the spine is proven in the B
  test with a dedicated 15-member lane.
- TUI stale-completion is proven at the state-transition layer
  (`TaskViewState` generation/project scoping,
  `WorkspaceDashboardState` generation/expansion/revocation guards),
  the same layer the M003/M004 command guards drive; no App-harness
  duplication was added.

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test --test work_orders_m007_trajectory
cargo test -p codegg-core --lib -- work_order
cargo test -p codegg --lib -- work_order
cargo test -p codegg --lib -- tool::task
cargo test -p codegg-protocol
cargo test --test work_orders_m001_foundation
cargo test --test work_orders_m002_materialization
cargo test --test work_orders_m004_dashboard
cargo test --features server --test work_orders_m005_trigger
cargo test --test work_orders_m006_agent_tool
cargo test --test identity_m003_daemon_authorization
cargo test --test subagent
cargo test --test storage_migrations
cargo test --test worktree -- work_order
cargo test --test tui -- work_order
cargo test --test tui_project_tabs
cargo test --test tui_project_picker
python3 scripts/check_authorization_matrix.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_task_trigger_boundaries.py
python3 scripts/check_work_order_coordinator.py
python3 scripts/check_tui_project_authority.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Plan §11 literal-to-actual mapping (recorded, not hidden):

- `cargo test -p codegg-core -- work_order`: literal filter matches the
  lib target only with `--lib`; substitute is
  `cargo test -p codegg-core --lib -- work_order` (43 passed; same
  substitution as M001-M006 closures).
- `cargo test -p codegg --lib -- work_order`: 37 passed (includes the
  7 `tool::work_order` unit tests).
- `cargo test --test authorization -- work_order`: no
  `tests/authorization.rs` target exists (same substitution as
  M001/M004/M005/M006 closures); substitutes are
  `identity_m003_daemon_authorization` (9, project auth/privacy) and
  `subagent` (22, TaskTool non-regression), plus the authorization
  matrix guard.
- `cargo test --test worktree -- work_order` / `cargo test --test tui
  -- work_order`: targets exist; zero `work_order`-filtered tests in
  them (14 and 165 filtered out respectively) — the worktree/TUI
  work-order surfaces are covered by M002/M003/M004/M007 suites instead.
- `cargo test --test tui_project_tabs` (20) and
  `cargo test --test tui_project_picker` (22) run in full per the plan.
- `cargo test -p codegg-protocol` (185): no DTO change in M007.
- Clippy ran with the repo-conventional `--locked` flag added.

### Results (local)

- M007 suite: 14 passed (A, B, C, D, E, F, G, H, I, J, K, L,
  15-plan trajectory, ownership audit).
- `codegg-core --lib work_order`: 43 passed. `codegg --lib
  work_order`: 37 passed. `tool::task`: 4 passed (TaskTool unchanged).
- `codegg-protocol`: 185 passed.
- M001 12 / M002 10 / M004 8 / M005 server 22 / M006 21 — no regressions.
- `identity_m003` 9 / `subagent` 22 / `storage_migrations` 4. All green.
- `tui_project_tabs` 20 / `tui_project_picker` 22. Green.
- Guards: authorization matrix verified; execution-ownership ok;
  daemon-cwd ok; git forbidden-patterns pass; task-trigger boundaries
  ok; work-order coordinator ownership ok; TUI project-authority ok;
  core-boundary pass; fmt clean; clippy
  (`--workspace --all-targets --locked`) clean; `verify.sh quick` pass.

## 5. Invariant review

| Plan §4 invariant | Evidence |
|---|---|
| Exactly one durable scheduler/admission owner | `ownership_single_scheduler_and_boundaries`; coordinator guard green (no `JobScheduler::new`, loop, bypass, test-runner, or copy-tree site); every initial turn through `JobSubmissionService` |
| At most one session + one initial turn per occurrence across retries/restarts/duplicate wakes | A/E/trajectory tests: deterministic `wo-session-`/`wo-turn-` identities, claim CAS, reconcile-by-key, exactly 1 job row |
| No fake Session rows for waiting work | A: session id derived only at materialization; M001 waiting rows have no session links |
| Post-materialization Session/Turn/AgentRun ownership | H: ordinary session row + sync permission registry + job-routed cancel; occurrence only records durable outcome |
| CAS-safe lane ordering; stale writes mutate nothing | B/K: revision conflicts with zero mutation; one-winner concurrent reorder |
| Failure/needs-attention holds downstream by default | B/trajectory: `HoldLane` holds on Failed/Cancelled/NeedsAttention; no mass release |
| Gate satisfaction latched per occurrence; duplicate signals harmless | C/I: latch-once + idempotent merge; trigger replay converges |
| Finite repeat with distinct identities stops exactly at bound | D: 1/2/256 exact, 257 rejected, exhaustion errors, duplicate-wake convergence |
| Policy cannot widen; stale model yields attention | G: narrowing minima; `model_unavailable` attention; post-claim immutability |
| Parallel Git mutations get distinct worktrees | F: 3 distinct managed worktrees/leases/paths under the managed root |
| Non-Git shared execution never labeled isolated | F: `isolation_unavailable` attention or `ShareSerialized` |
| Trigger tokens authorize only their one gate | I + M005 suite: non-Core fire path; bearer grants no principal authority |
| Agent WorkOrders bounded, no unbounded recursion | J + M006 suite: 16/8/4/depth/turn/project/descendant caps; restart-durable rows |
| Projections leak nothing unauthorized | K/L + M004 suite: not-found convention; privacy-filtered counts; revocation clears |
| Restart never replays committed side effects for metadata repair | E: recovery queries canonical stores first; links persist before next side effect |

## 6. Failure and recovery review

- Duplicate delivery: submission/batch/item/trigger-receipt keys
  converge; conflicting payloads conflict explicitly; distinct calls
  never dedupe on text alone.
- Stale writers: lane/work-order revision conflicts mutate nothing;
  trigger latch CAS admits one winner per occurrence.
- Concurrent batches/fires/reorders: exactly-one-winner semantics
  pinned in B/I/J/K.
- Restart: file-DB/in-memory reopen preserves identities, revisions,
  latches, deadlines, batch ledger, turn/project/descendant counters;
  incomplete-occurrence scan surfaces partial rows; running rows carry
  complete links.
- Partial persistence: batch/lane all-or-nothing (M001 transaction);
  occurrence links persist before each next side effect
  (workspace -> session -> job -> running).
- Malformed input: empty prompt, over-count/over-bytes/over-repeat,
  unknown gate kinds, immediate+delay mixes, overlong delays,
  cross-project bindings all fail closed with typed codes.
- Cancellation: pre-claim creates nothing; mid-materialization records
  the durable outcome; terminal-vs-cancel races resolve to the recorded
  terminal (idempotent); running cancellation routes through canonical
  job control first (coordinator path, H records the store outcome).
- Sequence: hold-by-default with explicit attention -> waiting retry
  and cancel/skip paths; `ContinueLane` opt-in preserved.
- Contention: per-turn/project/descendant/depth caps bound retries.

## 7. Migration and compatibility review

- No migration: M007 adds no tables/columns; `STORAGE_LAYOUT_VERSION`
  unchanged (`storage_migrations` 4 green).
- Protocol additive-stable: no new `CoreRequest`/`CoreResponse`/event
  variants; `codegg-protocol` 185 green.
- Tool-surface compatibility: `task` definitions unchanged
  (`tool::task` 4 + `subagent` 22 green); `work_order` tool untouched.
- Rollback: removing the M007 test file changes no runtime behavior.

## 8. Security review

- Authorization: project-scoped service calls throughout; ID-only
  lookups resolve owning project server-side; foreign-project access
  returns not-found (K).
- Privacy: bounded previews/summaries only in projections; trigger
  metadata/Debug shapes censused secret-free (I); denials leak no
  project/task/session existence.
- Secrets: bearer appears only in the one-time creation response;
  verifier-only storage retained (M005); no secret in events/audit/
  metrics/Debug (I).
- Isolation truthfulness: managed-worktree paths proven under the
  managed root with no shared write root (F); non-Git never isolated.
- Bounds as DoS control: batch/item/repeat/depth/turn/project/
  descendant caps re-pinned (J); cursor-bounded listings retained.
- Attribution: creator + parent lineage + invocation keys durable
  (M001/M006, re-verified by J reopen reads).

## 9. Documentation and operations

- `architecture/work_orders.md`: M007 section (per-matrix evidence,
  ownership pins, test commands, re-verified suite counts).
- Cross-doc audit (§12): `architecture/jobs.md`, `scheduler.md`,
  `session.md`, `authorization.md` (work-order + trigger capability
  tables), `tui.md` (`/workspace`, Task view), `server.md` (fire
  endpoint), `storage.md` (v60-v63), protocol docs, and README
  task/workspace commands were reconciled in M001-M006 and remain
  consistent with M007 findings — no contradictions found, no edits
  required beyond the M007 section.
- Operator diagnostics: unchanged from M001/M002/M005/M006 (typed
  `work_order_*` codes; `trigger_invalid`; bounded attention
  diagnostics; secret-free events/audit).
- Static guards: authorization matrix, work-order coordinator
  ownership, execution-ownership, daemon-cwd, git forbidden-patterns,
  task-trigger boundaries, TUI project-authority — all green.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No dedicated `tests/authorization.rs` for the plan's literal verification line | Substituted with named existing targets + guards (same surfaces, consistent with M001/M004/M005/M006 closures) | None; future plans should name existing targets |
| low | `check_scheduler_bypass.py` reports one finding in `src/agent/snapshot_capture.rs:181` (direct `.spawner().send(`) | Pre-existing file untouched by M001-M007 (this milestone's diff is tests + one arch doc); not part of `verify.sh quick`; no work-order path involved | None for M007; a future scheduler-ownership pass may annotate or reroute that call site |
| low | Daemon wake driving for non-trigger WorkOrders remains on explicit wake (carried from M002/M006) | Release-ready occurrences need an explicit coordinator wake; autonomous timer/predecessor wakes belong to a later hook | None; trajectories in this record drive explicit wakes (documented semantics) |
| low | Claim-time approval/sandbox ceilings remain `None` (carried from M002/M006) | Per-turn ceilings still enforced on `TurnSubmit`; snapshots never widen | Future milestone may thread ceilings into claim; unchanged behavior |
| low | Failed occurrences are terminal with no Failed -> attention transition (closed M001 matrix) | Retry uses the attention -> waiting path; predecessors that truly fail hold the lane until skip/cancel/advance | None; encodings documented in §3 |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and the workstream is fully closed: M007 exit
condition is satisfied (the complete WorkOrder/task-view capability
has closure evidence proving exactly-once materialization, one
scheduler owner, correct authority/isolation, recoverability, and
bounded UI/tool behavior through injected failures and restart).

Blocked-work audit (registry `Blocked work` + roadmap §6 dependency
graph):

- M007 (qualification, hard dep M001-M006) → **closed** by this
  record. All six hard dependencies were already closed (M001
  `a856f2e0`, M002 `c54656e5`, M003 `79a00460`, M004 `8c6e8190`,
  M005 `f22c9d8d`, M006 `38e533a8`); no interface dependency remains
  without a stable contract.
- No registered plan lists M007 (or the workstream) as a hard or
  interface dependency: dependency-security M005 remains independently
  blocked on the generalized external updater interface; architecture
  convergence M009 and runtime safety C002 remain conditionally closed
  on their own operational evidence. **Nothing is unblocked by this
  closure**; deferred product work stays intentionally unregistered.
- The Project Work Orders and Task View subsystem roadmap moves from
  `active` to `closed` (M001-M007 all closed).

## 12. Registry updates

- Move M007 (`007-work-order-trajectory-and-recovery-qualification.md`)
  from ready to closed with this closure record (implementation
  `0216e814`, harness-only, no production delta).
- Record M007 under recently closed work; update the subsystem roadmap
  M007 status to closed and the roadmap status to closed.
- Update the active-roadmap row, execution-order gate, blocked-work
  row, and control-point evidence to M001-M007 closed.
- No downstream plan moves to ready (audit in §11).
