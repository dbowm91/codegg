# Runtime Consolidation, Deletion, and Footprint M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-consolidation-deletion-footprint/001-legacy-background-scheduler-deletion.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Implementation commits:

- `9594429` — Remove legacy background scheduler
- `fcfed87` — Test legacy task request rejection

## 1. Executive finding

M001 is complete. The independent `BackgroundScheduler` timer, in-memory
task collection, task-table persistence interpretation, callback dispatch, and
UUID-to-`u64` bridge were deleted. Durable `ScheduleStore`/`JobStore` and the
existing scheduler remain the sole production scheduling owner. The legacy
`TaskList`, `TaskSchedule`, and `TaskDelete` wire variants remain source- and
wire-compatible but now return an explicit migration response directing
callers to the durable `Schedule*` protocol; they do not construct or retain a
second scheduler.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| No production `BackgroundScheduler` loop | Deleted `src/agent/task.rs`; removed all bootstrap wiring | pass | No production symbol or loop remains. |
| No UUID-to-`u64` scheduling path | Deleted `spawn_loop` and task ID parsing | pass | Repository source search found no legacy scheduling symbol. |
| One durable persistence interpretation | `CoreRuntimeDeps` retains only `JobStore`/`ScheduleStore`; migration imports old rows | pass | No new schema or task persistence path added. |
| Legacy requests do not create a second authority | `legacy_task_requests_are_explicitly_rejected` | pass | All three legacy variants return the explicit unsupported code. |
| Durable scheduling behavior remains intact | `cargo test -p codegg-core jobs::schedule`; scheduler focused tests | pass | 6 core schedule tests and 63 root scheduler tests passed. |
| Architecture/ownership docs describe the durable owner | `architecture/{agent,core,scheduler,tui}.md`, execution ownership files | pass | Removed stale scheduler ownership entries and inventories. |
| No new CI lane/schema/scanner | Diff and manifest review | pass | No workflow, migration, scanner, or scheduler framework added. |

## 3. Production implementation evidence

- Removed the entire legacy `src/agent/task.rs` module and its `agent::task`
  export.
- Removed `bg_scheduler` and `bg_scheduler_compat_enabled` from
  `LegacyAgentRuntimeDeps` and all constructors.
- Removed standalone, stdio, daemon-bootstrap, and local-TUI scheduler-loop
  construction from `src/main.rs`.
- Reduced the legacy task request surface to explicit unsupported responses;
  durable `ScheduleCreate`, `ScheduleList`, and `ScheduleDelete` handling is
  unchanged.
- Moved the small duration parser needed by legacy storage import and `/loop`
  command parsing into `background_task_migration`.
- Updated execution ownership and architecture documentation to remove the
  deleted implementation.

## 4. Verification executed

### Commands run

```bash
cargo check -p codegg --lib
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
cargo test -p codegg-core jobs::schedule -- --nocapture
cargo test -p codegg --lib background_task_migration -- --nocapture
cargo test -p codegg --lib scheduler -- --nocapture
cargo test -p codegg --lib legacy_task_requests_are_explicitly_rejected -- --nocapture
scripts/verify.sh quick
git diff --check
```

### Results

All listed focused tests and guards passed: 6 core schedule tests, 3
migration-parser tests, 63 scheduler tests, and 1 legacy-request regression
test. `scripts/verify.sh quick` passed, including workspace all-target checks.
An exploratory full `cargo test -p codegg --lib -- --test-threads=1` was
stopped after an existing long-running test produced no result for over three
minutes; this does not replace the focused evidence or quick verification.

## 5. Invariant review

- Production work remains scheduler-governed through the durable scheduler
  and submission services.
- Typed durable schedule and job identifiers are not translated through the
  removed legacy task representation.
- Restart/recovery remains in the durable schedule/job infrastructure; no
  detached timer survives daemon generation ownership.
- No direct subagent dispatch path was added.
- Legacy task requests cannot broaden authority because they are rejected
  before any storage or execution operation.

## 6. Failure and recovery review

There is no compatibility object to leave in memory after a failed durable
operation, no lock spanning an asynchronous legacy operation, and no detached
timer to race shutdown or restart. Durable schedule claim, job admission,
recovery, and resource semantics were not changed. Malformed/legacy requests
receive a bounded typed error response.

## 7. Migration and compatibility review

No schema migration was added. Existing legacy `task` rows remain handled by
the existing startup migration helper, which creates durable schedule records
and marks imported source rows; the runtime no longer interprets those rows as
live in-memory tasks. The three legacy request variants remain in the protocol
for wire compatibility but are explicitly unsupported and point callers to
the durable protocol. No public wire shape was changed.

## 8. Security review

The change removes an alternate execution authority and cannot grant new
permissions, paths, or process access. Durable scheduler admission and existing
session/workspace binding remain authoritative. No secrets, credentials, or
new filesystem paths were introduced.

## 9. Documentation and operations

Updated architecture ownership descriptions, TUI/core runtime inventories,
and the execution-ownership manifest/documentation. No new operator action,
schema migration, CI workflow, or static scanner is required.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Existing TUI task commands still issue retained legacy `Task*` requests and therefore receive the explicit unsupported response. | The old task UI is non-functional until migrated to `Schedule*` requests. | Treat as follow-up compatibility/UI work; it is outside M001 because changing the wire mapping would require a separate bounded protocol/UI slice. |

No critical, high, or medium finding remains in M001 scope.

## 11. Roadmap disposition

Milestone closed. M002, M004, and M005 remain ready. M003 remains blocked on
M002; M006 remains blocked on M002–M005; M007 remains blocked on M002–M006.
M001 closing therefore unblocks no registered future plan.

## 12. Registry updates

- Marked M001 closed in the subsystem roadmap.
- Removed M001 from the dependency-ready registry section.
- Added M001 to recently closed control points with implementation commits.
- Audited all blocked registry entries and their roadmap dependency graphs;
  no plan met its remaining hard-dependency conditions from M001 alone.
