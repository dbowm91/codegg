# TUI Live Work Visibility M002 — Scheduler Job Lifecycle and Snapshot Recovery

Status: **closed**
Primary class: invariant
Repository review baseline: `df923802` (`plan/tui-live-work-visibility-corrective`, rebased on `origin/main`, 2026-10-19)
Source roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md` M002.
Requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`, `#28-observability`, `#27-security-requirements`.
Related accepted decisions: ADR-0031 (idle completion), ADR-0038 (queue/scheduler authority); proposed ADR-0042 is **not** required here.
Corrective origin: `8fab1ba...` plus prior `plans/closure/tui-subagent-and-job-visibility-convergence/002-status.md`, which validated seeded `snapshot.jobs` rows but did not prove production publications.

## 1. Objective and readiness

Every accepted session-owned scheduler job, including a delegated subagent job admitted **directly** through `JobSubmissionService`, appears in the owner's `SessionProjectionSnapshot.jobs` with the correct pending/running/terminal state. Late attach and restart reconstruct outstanding jobs without waiting for another event. M002 is independent of TUI display changes, but M001 is the **soft** integration dependency for end-user validation.

## 2. Current evidence and bypass

`src/tool/task.rs` calls `submission.submit(NewJob {kind: Subagent, session_id: parent_session_id, ...})`; the located `CoreEvent::JobCreated` publication lives in `src/core/daemon_jobs.rs`'s `CoreRequest::JobSubmit` handling, not necessarily in this direct scheduler path. Trace *all* `JobSubmissionService::submit`, retry, claim, start, terminal and CI/watch/tool-program publishers before selecting an insertion point. `crates/codegg-core/src/projection_replay/publication.rs` builds `JobStarted/Completed/Failed` with `session_id: None` and an empty session envelope; `crates/codegg-protocol/src/projection/reducer.rs` scope-rejects foreign events. `src/core/daemon_refresh.rs::projection_snapshot_for_session` currently populates AgentRun, worktrees and groups but not outstanding jobs. `BackgroundJobCompleted` is a post-terminal hint, insufficient to establish liveness. Test the exact producer path; do not assume every job event is published to every subscriber.

## 3. Invariants / limits

- JobStore/Scheduler is the sole authoritative owner of job identity, owner session, workspace and lifecycle. Never infer session from current foreground tab, empty envelope, run label or caller-supplied display id.
- All admission paths emit equivalent lifecycle visibility, with exactly one logical row per `job_id` even through attempts/retries. Terminal updates must not resurrect a job as running.
- No scheduler admission/executor bypass, new polling job queue or frontend SQL reads. Workspace/user authorization and session filters must remain intact.
- Preserve bounded projection limits (currently `MAX_PROJECTION_JOBS`); select live jobs before recent terminals, and expose honest overflow/truncation rather than hiding pending work behind 32 terminal rows.
- Do not relabel `ToolProgram` or `AgentRun` as jobs without explicit persisted association; M004 owns dedupe/presentation.

## 4. Ordered work packages

A. **Red test matrix.** In SQLite-backed daemon harness, submit a `JobKind::Subagent` via `TaskTool`'s direct scheduler path; submit a normal background `bash` or `test` job, and a `CoreRequest::JobSubmit` control. Prove owner-scoped projected `JobCreated/Queued/Started/Terminal` gaps with initial and live subscribe. Capture canonical `job_id`, `session_id` and event sequence.

B. **Unify publication at real lifecycle owner.** Publish after durable admit/transition commit (not optimistic before); refactor `CoreRequest::JobSubmit` to reuse the same seam rather than double-emitting. For payloads that lack a `session_id`, resolve trusted owner from job store under daemon authority **before** issuing a projection-scoped event. Consider additive event metadata only if the producer cannot be made owner-scoped without it; record explicit compatibility effects. Protect against delayed/duplicate terminal hints and repeated attempts. Review `crates/codegg-protocol/src/projection/adapters.rs` and `crates/codegg-core/src/projection_replay/publication.rs` for consistent session scope/status vocabulary and existing `ProjectionReducer::JobUpserted` behavior.

C. **Rebuild authoritative initial state.** Extend `projection_snapshot_for_session` or an existing projection snapshot assembler using a bounded, indexed JobStore owner query. Include queued/blocked/running/backoff attempts, reconcile interrupted jobs on daemon restart and newest relevant terminals under declared cap. Resolve snapshot/event ordering with existing high-water cursor to prevent lost transitions or false liveness. No DB query in TUI render.

D. **Observability.** Emit bounded diagnostics for dropped events (missing owner, scope mismatch, retention, projection publication failure); never silently reinterpret as `no pending jobs`. Document what the initial snapshot does and does not include.

## 5. Failure, cancellation, restart, contention

Rapid created→started→completed; duplicate retry attempt; cancellation competing with start; late subscriber between snapshot assembly and stream cursor; crash after durable job commit before projection publication; recovery of prior-generation `running` as interrupted/requeued. Publish exactly at durable state boundary with replay/rebuild reconciliation; missing event must heal on next attach without claiming it has run. No long lock on scheduler dispatcher during projection assembly; bound DB scan and retained rows.

## 6. Security, compatibility and migrations

Reuse `CoreEvent::Job*`/`ProjectionEvent::JobUpserted` where possible; additive versioned fields if needed, old envelopes decode safely, never expose unscoped `JobStarted` onto arbitrary project stream. No storage migration expected solely for projection if existing job owner indexes suffice; if not, document schema/index migration and `STORAGE_LAYOUT_VERSION` implications. Do not change scheduler execution ownership.

## 7. Required tests and verification

Focused `codegg-core` job store and `codegg-protocol` adapters/reducer/replay; integrated TaskTool→SubmissionService→Scheduler→EventLog→projection snapshot **without** manually publishing `JobCreated`; normal `JobSubmit` parity; owner-vs-other session deny; late attach, restart, duplicate/reordered retry, cancelled and terminal capping. One test must assert that deleting the scheduler-wide lifecycle publisher breaks real visible-job convergence. Confirm `JobGet/List/Wait` agree with projected live state.

Execute relevant `cargo test -p codegg-core --locked`, `cargo test -p codegg-protocol --lib projection --locked`, `cargo test -p codegg --test projection_transport_real --locked` and new integrated target; projection transport guards, `scripts/check_scheduler_bypass.py`, `scripts/check_execution_ownership.py`, fmt, supported clippy, `scripts/verify.sh quick`, `git diff --check`. Update `architecture/jobs.md`, `architecture/projection.md`, `architecture/scheduler.md` and applicable docs.

## 8. Acceptance, stop, closure

Pass: direct-delegation and ordinary background jobs show queued/running and terminal under the *correct* authorized session; late attach reconstructs them; retry/cancel/restart behave monotonically. Stop if a cross-session/global publication design or new independent scheduler is required, or durable owner is genuinely unavailable; record blocking ADR rather than guessing. Closure `plans/closure/tui-live-work-visibility-corrective/002-status.md`: producer inventory, request/event/owner proof, fail-before/pass-after test, high-water/recovery and privilege negatives, exact commands/commits and residual risks.
