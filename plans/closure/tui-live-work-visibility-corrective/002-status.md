# TUI Live Work Visibility Corrective M002 — Closure Status

Status: **closed**
Date: 2026-10-19
Class: **invariant**
Source implementation plan: `plans/implementation/tui-live-work-visibility-corrective/002-scheduler-job-publication-and-snapshot-recovery.md`
Source subsystem roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md#7-milestones`
Repository baseline reviewed: `df923802` (rebased target branch)
Implementation commit: `a7774c1e7bc23047bac3076ee236e9f26b2b39f0` — owner-scoped admission/lifecycle publication and snapshot recovery

## 1. Executive finding

Every scheduler admission path that uses the daemon's `JobSubmissionService`
now shares one post-commit `JobCreated` publication seam. Scheduler-owned
started and terminal transitions resolve the durable `JobRecord` before
publication, so event ownership comes from the authoritative store. Session
projection snapshots reconstruct bounded live jobs before recent terminals,
allowing a late subscriber or restarted daemon to recover work without waiting
for another event.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Direct service admission publishes exactly one owner-scoped create | `same_submission_key_creates_one_durable_job` | pass | The first durable submission emits one record; an idempotent retry does not emit another. |
| Real socket subscriber sees a post-subscribe scheduler admission | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job` | pass | The test submits through `JobSubmissionService` after the real TUI subscription is installed and applies the resulting projected row. |
| Session snapshots reconstruct live jobs with the correct owner | `session_projection_rebuilds_owner_scoped_live_jobs` | pass | A second session's durable job is excluded from the requested snapshot. |
| Start, terminal, retry, cancel, timeout, and interruption map to lifecycle upserts | protocol adapter and `job_lifecycle_projection_uses_the_trusted_event_envelope_owner` tests | pass | Ownerless lifecycle events are dropped; owner comes from the trusted event envelope produced after durable owner lookup. |
| Duplicate and reordered lifecycle hints do not regress state | `job_upsert_preserves_lifecycle_order_and_retry_attempts` | pass | Delayed create/start hints cannot reopen terminal state; a new retry attempt may reopen it. |
| Live jobs take precedence over retained terminal history and overflow is visible | bounded owner query in `projection_snapshot_for_session` | pass | Live query is capped at `MAX_PROJECTION_JOBS + 1`; truncation and read failure produce bounded diagnostics. |
| Normal request and direct tool paths share the publisher | `CoreRequest::JobSubmit` and `TaskTool` both use `JobSubmissionService`; duplicate handler publication removed | pass | The real socket regression exercises the shared durable service seam. |

## 3. Production implementation evidence

`JobSubmissionService` forwards the committed `JobRecord` through a bounded
channel after durable creation and successful scheduler enqueue. It awaits
bounded-channel capacity so a full queue does not silently drop a live create;
a closed receiver is logged and the next authoritative snapshot rebuilds from
`JobStore`. `CoreRequest::JobSubmit` no longer publishes a duplicate.

The daemon owns one scheduler lifecycle receiver. Admission and release events
resolve `JobRecord` by job ID before creating owner-scoped `CoreEvent`s. Queued
cancellation without an attempt has a terminal event. Projection adapters drop
ownerless lifecycle events instead of globalizing them. Snapshot recovery
queries by session, includes scheduled/queued/running/blocked states first, and
fills remaining capacity with recent terminal jobs.

## 4. Verification executed

| Command | Result |
|---|---|
| `rtk bash scripts/verify.sh quick` | pass, local working tree |
| `CARGO_BUILD_JOBS=2 rtk cargo clippy --workspace --all-targets --locked -- -D warnings` | pass, local working tree |
| `rtk cargo test -p codegg --lib same_submission_key_creates_one_durable_job --locked` | pass (1) |
| `rtk cargo test -p codegg --lib session_projection_rebuilds_owner_scoped_live_jobs --locked` | pass (1) |
| `rtk cargo test -p codegg --lib default_tui_socket_path_installs_authorized_session_snapshot_and_live_job --locked` | pass (1) |
| `rtk cargo test -p codegg-core job_lifecycle_projection_uses_the_trusted_event_envelope_owner --locked` | pass (1) |
| `rtk cargo test -p codegg-protocol --lib projection --locked` | pass (96) |
| `rtk python3 scripts/check_scheduler_bypass.py` | pass |
| `rtk python3 scripts/check_execution_ownership.py` | pass |
| `rtk python3 scripts/check_projection_transport_isolation.py` | pass |
| `rtk python3 scripts/check_projection_transport_lifecycle.py` | pass |
| `rtk git diff --check` and `rtk cargo fmt --all -- --check` | pass |

These are local receipts; no hosted CI run is claimed.

## 5. Invariant review

- `JobStore`/scheduler remain authoritative for job identity, ownership, and
  lifecycle; the TUI performs no storage query.
- Direct tool submission and request submission share the same durable create
  boundary; retries are deduplicated by the existing submission key.
- Ownerless events are refused, and another session's job is excluded from the
  snapshot query.
- Snapshot row caps remain bounded and live jobs are selected before terminal
  history.
- No execution path or scheduler authority was added or bypassed.

## 6. Failure and recovery review

The event receivers are bounded and daemon-owned. A full admission queue
backpressures the post-commit observer until capacity is available; durable
submission itself is not rolled back. If the receiver has closed, the warning
is explicit and the durable snapshot remains the recovery authority. A crash
between durable admission and event publication is healed on the next attach
or resync. Snapshot assembly uses bounded indexed owner queries, not render-time
reads.

## 7. Migration and compatibility review

No DTO or storage migration was introduced. Existing `JobStore` owner indexes,
`CoreEvent::Job*`, and `ProjectionEvent::JobUpserted` are reused. Existing
terminal notification history remains intact. Empty lifecycle metadata is
merged with previously known job metadata rather than erasing it.

## 8. Security review

Session ownership is loaded from the durable record for scheduler transitions.
Ownerless events are dropped with a diagnostic. The snapshot query is scoped
to the authorized session, and an explicit two-session fixture proves the
other session's job is absent.

## 9. Documentation and operations

Updated `architecture/jobs.md`, `architecture/projection.md`,
`architecture/scheduler.md`, `.opencode/skills/jobs/SKILL.md`, and
`.opencode/skills/scheduler/SKILL.md` with the publication, bounded queue,
owner lookup, monotonic merge, and recovery contracts.

## 10. Unresolved findings

No high- or medium-severity finding remains for M002. The real socket test
exercises live admission; lifecycle event mapping and retry ordering are
covered by protocol/store tests rather than a long-running process fixture for
every executor terminal outcome. A failed observer receiver is recovered by
the next authoritative snapshot and is logged, not represented as an empty
job set.

## 11. Roadmap disposition

M002 is closed. M001 is also closed. M003 remains blocked because proposed
ADR-0042 requires explicit acceptance before its child-identity/schema work;
therefore M004, M005, and M006 remain blocked in order.

## 12. Registry updates

The M002 implementation plan and roadmap row are marked closed and this record
is listed under Recently closed. The unblock audit found no newly eligible
downstream plan: M003 still requires ADR-0042 acceptance, and M004–M006 retain
their hard dependency chain.
