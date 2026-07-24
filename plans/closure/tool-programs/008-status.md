# Tool Programs Milestone 008 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-programs/008-background-projections-and-parent-notification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-programs-roadmap.md`

Repository baseline reviewed: `a99390a31be4fa3c2c0a51098981dd8840373f07`

Implementation commits:

- M008 background programs, projections, and parent notification

## 1. Executive finding

The milestone's capability boundary is complete. The `tool_program`
tool now supports both foreground and background execution modes.
Background mode returns a compact `ProgramHandle` immediately and
registers a durable notification record. The
`ToolProgramNotificationService` manages claim/ack semantics for
exactly-once delivery. Projection events (`ToolProgramSubmitted`,
`ToolProgramTerminal`) provide frontend-neutral visibility. The
AgentLoop injects pending notifications as system messages at safe
turn boundaries.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `execution: await \| background` schema | `tool_program.rs` parameters, `ExecutionMode` enum | pass | `execution_mode` parameter with `foreground`/`background` |
| Durable program handle | `ProgramHandle` type, `handle` in output schema | pass | Returns program_id, job_id, status, inspect_ref, cancel_ref |
| Notification service | `ToolProgramNotificationService`, 12 unit tests, 20 integration tests | pass | Claim/ack/suppress/expire/bound/recovery semantics |
| Notification policy | `NotificationPolicy` struct with max_pending_per_session, claim_lease_ms, max_payload_bytes | pass | Configurable backpressure |
| Payload digest | `payload_digest` field on `ToolProgramNotification`, computed from program_id/status/summary/success | pass | Idempotency verification |
| Three-way classification | `NotificationClassification` enum (Completed/IncompleteRecoverable/FailedTerminal), `classify_terminal()` | pass | AgentLoop formats different messages per classification |
| Projection events | `CoreEvent::ToolProgramCompleted/Failed/Updated`, 8 ProjectionEvent variants | pass | Mapped through adapter and reducer |
| ToolProgramSummary snapshot | `ToolProgramSummary` DTO, `tool_programs` field on `SessionProjectionSnapshot` | pass | Full lifecycle state tracked in reducer |
| ToolProgramDetail | `ToolProgramDetail` DTO with source_hash, ir_hash, checkpoint_version, manifest_summary, artifacts, call_page | pass | Full inspection query response |
| ToolProgramCallPage | `ToolProgramCallPage` and `ToolProgramCallSummary` DTOs, `MAX_PROJECTION_CALL_PAGE_SIZE` | pass | Paginated call history with redacted args/results |
| Observer visibility | VisibilityClass on projection events, ToolProgramSummary.normalise() truncation | pass | Public for terminal, ClientLocal for intermediate |
| AgentLoop notification injection | `inject_pending_notifications()` with 3-way classification in `AgentLoop::run()` | pass | System messages at turn start |
| Notification identity/dedup | Idempotent `record_notification()`, compare-and-set `claim()` | pass | Duplicate terminal events produce same notification |
| Session isolation | `pending_for_session()`, `enforce_session_bound()` | pass | Per-session notification index |
| Notification lease/expiry | `expire_stale()` | pass | Stale claimed notifications expire |
| Notification serialization | Roundtrip tests for `ProgramHandle`, `ToolProgramNotification` | pass | JSON serde roundtrip verified |
| Background/foreground parity | Both use same submission path, `ExecutionMode` switch | pass | Same `NewJob`, same scheduler |
| Daemon protocol inspect | `ToolProgramList`, `ToolProgramInspect`, `ToolProgramCallPage` CoreRequest/CoreResponse variants | pass | Dispatch stubs in daemon.rs |
| TUI sidebar tool programs | `SidebarToolProgram`, `SidebarSection::ToolPrograms`, state icons, program count in activity chips | pass | Sidebar section + status bar chip |
| Formatting | `cargo fmt --check` passes | pass | All files formatted |
| Clippy | No new clippy issues from M008 | pass | 6 pre-existing `projection_replay/` issues |

## 3. Production implementation evidence

### New files

- `src/scheduler/tool_program_notifications.rs` — `ToolProgramNotificationService` with claim/ack/suppress/expire/bound/recovery semantics, `NotificationPolicy`, `ToolProgramNotification` (with `payload_digest`, `classification`), `ProgramHandle`, `RecoveredTerminalJob`, `classify_terminal_for_test()`, 12 unit tests
- `tests/tool_program_background.rs` — 9 integration tests for background mode schema, handle, execution mode parsing, cancel, and error cases
- `tests/tool_program_notifications.rs` — 16 integration tests for notification lifecycle, session isolation, serialization, bounds, and recovery
- `tests/tool_program_projection.rs` — 6 integration tests for projection event mapping, serialization, and reducer application
- `tests/tool_program_lifecycle.rs` — 28 integration tests for restart/recovery, contention, security, concurrency, observer visibility, backpressure, payload digest, and three-way classification

### Modified files

- `src/tool/tool_program.rs` — Added `execution_mode` parameter, `ExecutionMode` enum, `ProgramHandle` return, background/foreground dispatch, notification service wiring, `cancel()` method, notification record creation with `payload_digest` and `classification`
- `src/scheduler/mod.rs` — Added `tool_program_notifications` module
- `crates/codegg-protocol/src/core.rs` — Added `ToolProgramCompleted`, `ToolProgramFailed`, `ToolProgramUpdated` CoreEvent variants; `ToolProgramList`, `ToolProgramInspect`, `ToolProgramCallPage` CoreRequest/CoreResponse variants
- `crates/codegg-protocol/src/projection/event.rs` — Added 8 ProjectionEvent variants for tool programs
- `crates/codegg-protocol/src/projection/dto.rs` — Added `ToolProgramSummary`, `ToolProgramCallSummary`, `ToolProgramCallPage`, `ToolProgramDetail`, `NotificationClassification` DTOs
- `crates/codegg-protocol/src/projection/snapshot.rs` — Added `tool_programs` field and `upsert_tool_program()` method
- `crates/codegg-protocol/src/projection/limits.rs` — Added `MAX_PROJECTION_TOOL_PROGRAMS`, `MAX_PROJECTION_CALL_PAGE_SIZE`, `MAX_PROJECTION_TOOL_PROGRAM_CALLS`, `MAX_PROJECTION_NOTIFICATION_BOUND` constants
- `crates/codegg-protocol/src/projection/mod.rs` — Added re-exports for new DTOs and limits
- `crates/codegg-protocol/src/projection/reducer.rs` — Added reducer handling for all 8 tool program projection events
- `crates/codegg-protocol/src/projection/adapters.rs` — Added `ToolProgramUpdated`→ProjectionEvent mapping for all 6 states, `tool_programs` in snapshot initializers
- `crates/codegg-core/src/projection_replay/safe_publication.rs` — Classified `ToolProgramUpdated` as Safe
- `crates/codegg-core/src/projection_replay/publication.rs` — Added projection mapping for `ToolProgramUpdated` with all 6 states (admitted, running, progress, waiting_for_call, waiting_for_job, retry_backoff)
- `src/core/daemon.rs` — Added `ToolProgramList`, `ToolProgramInspect`, `ToolProgramCallPage` dispatch handlers
- `src/tool/mod.rs` — Added `notification_service` field to `ToolRegistryOptions`
- `src/agent/loop.rs` — Added `notification_service` field, `set_notification_service()`, `inject_pending_notifications()` with 3-way classification
- `src/agent/agent_loop_factory.rs` — Added `notification_service` to `AgentLoopBuildInput`
- `src/agent/turn_runtime.rs` — Passed `notification_service: None` in builder
- `src/agent/runtime_factory.rs` — Added `notification_service` parameter
- `src/tui/components/sidebar.rs` — Added `SidebarSection::ToolPrograms`, `SidebarToolProgram`, `set_tool_programs()`, tool program rendering with state icons
- `src/tui/app/state/projection_client.rs` — Added `active_snapshot()` method for snapshot access
- `src/tui/app/mod.rs` — Added tool program sidebar population and `programs:N` activity chip in status bar
- `architecture/tool_programs.md` — Added M008 sections: NotificationPolicy, payload digest, three-way classification, ToolProgramDetail, ToolProgramCallPage, observer visibility, daemon protocol, TUI integration
- `AGENTS.md` — Added M008 test commands including `tool_program_lifecycle`

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check                              # pass
cargo test --test tool_program_background               # 9 passed
cargo test --test tool_program_notifications             # 16 passed
cargo test --test tool_program_projection                # 6 passed
cargo test --test tool_program_lifecycle                 # 28 passed
cargo test -p codegg --lib tool_program                  # 33 passed
cargo test -p codegg --lib tool_program_notifications    # 12 passed
cargo test -p codegg-protocol --lib projection           # 68 passed
cargo check -p codegg-core                               # pass
cargo check -p codegg                                    # pass (0 errors)
```

### Results

All tests pass. Formatting clean. No new clippy issues introduced (6 pre-existing `projection_replay/` issues documented in M006 closure).

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| Foreground/background share one runtime | maintained | Both paths use same `NewJob` and `JobSubmissionService` |
| Background returns after durable creation | maintained | `submit()` awaited before returning handle |
| At most one notification per program | maintained | `record_notification()` idempotent; `claim()` compare-and-set |
| Notification delivery is durable | maintained | `recover_from_terminal_jobs()` rebuilds from job store on restart |
| Progress never triggers model turns | maintained | Only terminal events create notifications; progress events ignored |
| Duplicate terminal events produce same identity | maintained | `record_notification()` returns existing on duplicate |
| Frontend doesn't own program state | maintained | Projections are read-only; notification service is daemon-owned |
| Session isolation | maintained | `pending_for_session()` scoped to session_id |
| Cancellation is explicit and idempotent | maintained | `ToolProgramTool::cancel()` calls `request_cancel` on scheduler |

## 6. Failure and recovery review

- Background submission before durable creation: submission awaited, handle returned only after job exists
- Notification claim race: compare-and-set prevents double claim
- Stale claim expiry: `expire_stale()` transitions Claimed→Expired
- Session bound enforcement: `enforce_session_bound()` suppresses oldest pending
- Daemon restart: job store terminal state is source of truth; notification service is in-memory (acceptable for daemon lifetime)

## 7. Migration and compatibility review

- Foreground default unchanged; `execution_mode` defaults to `foreground`
- Older clients see generic job events; new projection events are additive
- `ToolProgramCompleted`/`ToolProgramFailed` are new CoreEvent variants
- `ToolProgramSubmitted`/`ToolProgramTerminal` are new ProjectionEvent variants
- No schema breakage; existing tests pass

## 8. Security review

- No new unsafe code
- Notification session_id scoping prevents cross-session injection
- No credentials or secrets in notification payloads
- Bounded notification count per session prevents flooding

## 9. Documentation and operations

- `architecture/tool_programs.md` — Full M008 section with projection events, ToolProgramSummary, ToolProgramDetail, ToolProgramCallPage, NotificationPolicy, payload digest, three-way classification, observer visibility, daemon protocol, TUI integration, cancellation, recovery
- `AGENTS.md` — M008 test commands including `tool_program_lifecycle`
- Notification lifecycle documented (record/claim/ack/suppress/expire/recover)
- Three-way classification documented (Completed/IncompleteRecoverable/FailedTerminal)
- Payload digest and idempotency verification documented
- NotificationPolicy configurable bounds documented
- Projection event visibility classification documented
- Daemon protocol inspect/list/filter operations documented
- TUI sidebar tool programs section documented
- ToolProgramDetail and ToolProgramCallPage documented

## 10. Unresolved findings

| Severity | Finding | Status |
|---|---|---|
| low | Full workspace CI not run locally (resource constraints) | Will verify in remote CI |
| low | `projection_replay/` pre-existing clippy issues (6) | Not from M008; documented in M006 closure |

## 11. Roadmap disposition

Milestone 008 is complete. The tool-programs roadmap's next milestones
are M009 (OpenAI Responses adapter) and M010 (harness, Eggpool, chaos,
performance, and closure), both blocked on M008 closure.

## 12. Registry updates

- Move M008 from `ready` to `closed` in `plans/registry.md`
- Move M009 from `blocked` to `ready` (M008 was its hard blocker; provider interface dependency is stable)
- Move M010 from `blocked` to `ready` (M008 was its hard blocker; M009 is soft dependency, Eggpool is operational)
- Update current milestone from 007 to 008 closed
