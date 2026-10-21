# Eggwork Worker M001 — Expose Explicit Remote Job and Schedule Targets

Status: **implemented**
Class: capability / protocol / scheduler
Source: `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`
Reviewed baseline: CodeGG `main` (2026-10-19); relevant current source `crates/codegg-protocol/src/dto.rs`, `crates/codegg-core/src/protocol_conversions.rs`, `crates/codegg-core/src/jobs/schedule.rs`, `src/core/daemon_jobs.rs`, `src/scheduler/eggwork.rs`, `src/scheduler/submission.rs`.
Existing closed evidence: `plans/closure/eggwork-fixed-target-remote-execution/003-status.md`.

## 1. Objective

Expose the already-implemented `ExecutionTarget::EggworkNode { node_id }` through ordinary authenticated CodeGG job submission and durable schedule creation/materialization, without enabling unsupported job kinds or bypassing existing scheduler/authorization boundaries.

## 2. Verified gap

`JobSubmitDto` lacks a target and `job_submit_from_dto` forcibly defaults to Local; `JobTemplate` lacks a target despite persisted `NewJob/JobRecord` supporting one. Existing remote executor eligibility is limited to Build/Lint/Format/ManagedProcess/Shell. Existing TUI schedule creation primarily materializes subagents, so adding a target must not imply remote subagents are supported. Node credentials and endpoints are daemon-owned config.

## 3. Work packages

A. Add an optional, version-safe typed target to `JobSubmitDto`, DTO/domain conversions, the protocol clients, and relevant job serialization/readback. Decode omitted/legacy target as Local and reject invalid tagged unions, empty node IDs, unknown kinds or unrestricted key/path overrides. Remote target must reference configured node identity, not a raw endpoint.

B. Extend `JobTemplate` with a serde-defaulted target and ensure schedule create/get/list/restart/materialization preserves it and passes it unchanged into durable `NewJob`. Audit both in-memory and SQLite materializer paths and schedule-occurrence transactional idempotency. Existing persisted templates without target continue to materialize Local. A schema migration is justified only if actual stored representation requires one; document why if so.

C. Enforce node-target authorization **at host submission and schedule-creation** boundaries (not only when executor validates later). Determine existing role/workspace/project auth, approved node allowlist and execution policy ceiling; if existing authorization cannot express approval for remote execution, introduce the smallest typed operation/grant, fail closed by default and document its administrator configuration. A schedule created by an authorized principal must not silently elevate if node permissions later change: establish explicit creation-time validation plus execution-time policy recheck. Never embed TLS credentials in durable job fields or logs.

D. Validate remote eligible `JobKind` and typed command argv coherency before enqueue. Reject remote `Test`, AgentTurn, Subagent, Python, ToolProgram and unhandled kinds with actionable typed error, without creating a local job. Ensure a daemon with zero configured remote nodes rejects remote submission but keeps local behavior. Foreground/agent tool UX may add an explicit named-node selection surface if minimal; no cluster selection.

E. Extend job/schedule introspection with target identity (no endpoint/key), preserving old wire clients' optional fields. Verify existing `JobGet`, wait, status, cancel and terminal notice work without a second remote-job registry.

## 4. Acceptance tests

- Old `JobSubmitDto` and old persisted `JobTemplate` deserialize/materialize as Local; local schedule and TUI tests unchanged.
- Explicit named target survives protocol roundtrip, `JobStore` persistence, daemon restart and due occurrence; correct Eggwork executor receives exactly one admitted attempt.
- Unknown/unauthorized node, remote Test/Subagent, malformed target, invalid policy, missing node credentials fail closed (and no fallback/implicit local job).
- One-shot and interval schedules target Eggwork, overlap/missed-run do not duplicate; restart/replay preserves schedule-to-job target and idempotency.
- Cancel queued/running remote job and observe terminal result using the existing Job API; authenticated real-node mTLS fixture verifies execution and RunStore.
- Static routing ownership guards still detect a bypass; explicitly test that no frontend/tool locally spawns a remote-targeted command.

## 5. Verification and documentation

Focused: `cargo test -p codegg-core --locked`; `cargo test -p codegg-protocol --locked`; `cargo test --test eggwork_remote_execution --locked`; `cargo test --test eggwork_remote_execution_live --locked` on supported Linux fixture. Run `python3 scripts/check_eggwork_target_routing.py`, `python3 scripts/check_scheduler_bypass.py`, `python3 scripts/check_execution_ownership.py`, `scripts/verify.sh quick`; broader suite per `AGENTS.md`.

Update `architecture/jobs.md`, `architecture/scheduler.md`, `architecture/config.md`, protocol docs and operator usage. Record exact input grants, migration decision, tested OS, and an implementation/closure matrix in `plans/closure/eggwork-worker-telemetry-and-targeting/001-status.md`.

## 6. Stop conditions

Stop if delivering remote target requires making arbitrary host URLs a user-level credential, routing around `JobSubmissionService`, weakening per-node isolation requirements, or inferring remote-test/agent support from generic target fields. If schedule materialization cannot durably keep target without a migration or new authorize-at-trigger design, document blocker rather than fallback to Local.
