# Agent Loop Operational Reliability M005 — Subagent Admission and Failure Visibility

Status: active

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main; verify actual starting head)

Source roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`; `plans/000-long-term-specification.md#29-system-invariants`; `plans/001-terminology-and-domain-model.md#7-agent-terms`.

Applicable ADRs: No new ADR. Consume ADR-0002 parent/child isolation and existing durable AgentTask/AgentRun, scheduler and Worktree owner contracts.

Primary class: infrastructure

## 1. Objective

Eliminate avoidable subagent creation failures caused by transient SQLite snapshot-lock contention, keep spawn idempotent across attempts, and make durable terminal child errors actionable to the parent through the standard `task` surface and completion notification.

## 2. Why this milestone is ready

Agent-run stores, SQLite pool, `TaskTool`, control/status/wait paths, scheduler and projection are present. Previous long-horizon, agent-run and worktree milestones are already closed; agent-workstream-blackboard shared-write work is independent and not a prerequisite.

## 3. Current implementation evidence

- Reported test: one child fails during spawn with SQLite extended error 517 `SQLITE_BUSY_SNAPSHOT`; another queued but its provider rejected tool deferral. M005 owns the storage/recovery/parent-visible side, M006 owns provider wire compatibility.
- `src/tool/task.rs` forwards `agent_runs.create_or_get` errors directly; uses `NewAgentTask`, `NewAgentRun` and delegation identity; `spawn_many` loops child requests serially and records rejected child errors.
- `task(status|wait)` requires durable `run_id`; `get` with durable `task_id` yields terse `Task / Run / Status / Result` and legacy numeric fallback. Child failure details are not available in the shown formatter.
- `src/core/daemon_construct.rs` retries `SQLITE_BUSY` for projection publication, but that is not proof the `AgentRunStore` creation transaction does so. Existing queue corrective established that deferred-read-then-write transactions may fail to upgrade and need transaction-shape repair (`BEGIN IMMEDIATE`).

## 4. Invariants

Every creation has durable canonical identity and idempotency key; no duplicate child, worktree, job or side effect on transient retry. Parent/child authority ceiling, workspace isolation, depth and admission budgets never widen. Wrong-session run details remain inaccessible. Durable terminal status must not silently turn into success or disappear.

## 5. Scope

In: classify and reproduce busy-snapshot during durable child creation; correct atomic transaction shape/retry for the whole operation; child status/result/ref plus bounded typed error details in `task` and parent notifications; consistent `task_id`/`run_id` UX.

Out: scheduler replacement, arbitrary fan-out increases, automatic worktree integration or merge, sibling unrestricted chat, new SQLite pool tuning without root-cause evidence.

## 6. Required production changes

**Storage:** inspect implementation of `AgentRunStore::create_or_get` and its constituent transaction boundaries. Use `BEGIN IMMEDIATE` on read-then-write paths where necessary and/or retry the whole idempotent transaction after a fresh snapshot for extended codes 5/517 (not arbitrary SQL errors). Use bounded exponential or modest jittered backoff and deadline; do not retry one post-write statement independently. Tests must show no double-insertion after ambiguous commit.

**Task/run:** preserve source error class, stage (`create`, `admit`, `execute`, `provider`, `cancel`), retryability and short redacted diagnostic in durable state using existing fields/records if possible. If persistence is unavailable, publish bounded transient error but do not misrepresent it as terminal child state. Support direct parent retrieval of failure details and exact handle without needing daemon log access. Return usable guidance when `task_id` vs `run_id` is supplied to the wrong action, without guessing authorization.

**Scheduler:** child queueing and journal append remain canonical and idempotent. If a child reached scheduler admission, a transient parent tool timeout cannot create a second child on retried submission.

**Security/protocol:** additive optional failure metadata only, session/project authorization before detail fetch, scrub secrets, keep older DTO consumers reading previous fields.

## 7. Ordered work packages

A. Reproduce error 517 with two concurrent spawn/queue writers in WAL against a local test database, trace the exact transaction and extended SQLite code; demonstrate failure at baseline.

B. Fix transaction/retry at single durable store owner and test commit ambiguity/idempotent duplicate submits, cancellation during backoff, finite retry exhaustion.

C. Add child failure envelope to `task status/wait/get` and existing notification path; preserve child identity across failure; differentiate provider error from storage admission error.

D. Parent-loop replay fixture spawns two children, waits via scheduler notifications (not busy polling), gets actionable failure/valid completion and can replan without becoming `EquivalentError` stall.

## 8. Failure, cancellation, restart, and contention

On `BUSY_SNAPSHOT`, rollback/restart entire txn; on persistent busy after cap report typed transient admission failure and consistent no-or-one child state. On crash after commit, resumed submission converges to the existing child. Cancellation before enqueue leaves no orphan; after enqueue goes through standard cancellation. Parent loses no completed failure notification on restart; child failure cannot leak across sessions.

## 9. Compatibility and migration

Prefer existing SQLite schemas and result refs; if a new failure-reason column or event is required, make it additive and bump schema/layout in the canonical migration path with old-row default handling. Existing numeric legacy IDs and UUID durable IDs must remain distinct without silent coercion.

## 10. Required tests

Focused: AgentRun SQLite store create-or-get concurrent transaction tests, `src/tool/task.rs` formatting/argument schema; rich busy code classifier and retry deadline. Integration: `spawn`/`spawn_many` and `status`/`wait`/`get` returning terminal failed run details, exact parent actor scope, group partial success, no second child. Restart: crash-after-commit idempotent replay and durable notification. Contention/cancel: 8+ concurrent spawns under WAL, long reader, cancel during backoff. Security: cross-session run ref, secret-bearing provider failure, invalid run/task IDs, parent-child ceiling, terminal status during retry.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

First run focused test filters for the named modules (`cargo test -p codegg --lib <filter> --locked`) and existing integration targets; confirm actual crate/target names at execution time. Record exact commands, stdout/result and head in closure. Hosted checks only where the existing closure policy requires them. No live-provider CI or new scheduled test lane.

## 12. Documentation updates

Update only affected `architecture/` and `docs/` surfaces; record any compatibility/config changes in `docs/tools.md` or `architecture/provider.md` where applicable. Keep existing predecessor closure history immutable.

## 13. Acceptance criteria

1. Deterministic concurrent fixture that previously hit 517 now converges to exactly-once durable child creation or a documented finite retry failure without orphan/duplicate.
2. Parent can see a failed child, cause/stage/retryability and exact run/task identifiers via an authorized `task` read without daemon log inspection.
3. Provider-side failures remain visible but are not silently fixed by storage retry; M006 owns their preflight/wire resolution.
4. Cancellation, restart, spawn-group partial acceptance and worktree ceilings remain correct.

## 14. Stop conditions

Stop and record a blocker if implementation would need a new authority owner, broad sandbox changes, unsafe command execution in tests, an undocumented provider wire workaround, a nonadditive migration, or an unaccepted architectural decision. A test-only mitigation with a production gap is not closure.

## 15. Handoff

At start reconcile this plan against actual code and exact head. Preserve unrelated changes. Commit scoped production/tests/docs and add `plans/closure/agent-loop-operational-reliability/NNN-status.md` with requirement/evidence table, exact commands and outcomes, restart/contention/security proof, and residuals. Update registry and roadmap to reflect actual closure or blocker only.
