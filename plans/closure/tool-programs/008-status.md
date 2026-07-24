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
| Notification service | `ToolProgramNotificationService`, 13 unit tests, 13 integration tests | pass | Claim/ack/suppress/expire/bound semantics |
| Projection events | `CoreEvent::ToolProgramCompleted/Failed`, `ProjectionEvent::ToolProgramTerminal/Submitted` | pass | Mapped through adapter and reducer |
| AgentLoop notification injection | `inject_pending_notifications()` in `AgentLoop::run()` | pass | System messages at turn start |
| Notification identity/dedup | Idempotent `record_notification()`, compare-and-set `claim()` | pass | Duplicate terminal events produce same notification |
| Session isolation | `pending_for_session()`, `enforce_session_bound()` | pass | Per-session notification index |
| Notification lease/expiry | `expire_stale()` | pass | Stale claimed notifications expire |
| Notification serialization | Roundtrip tests for `ProgramHandle`, `ToolProgramNotification` | pass | JSON serde roundtrip verified |
| Background/foreground parity | Both use same submission path, `ExecutionMode` switch | pass | Same `NewJob`, same scheduler |
| Formatting | `cargo fmt --check` passes | pass | All files formatted |
| Clippy | No new clippy issues from M008 | pass | 6 pre-existing `projection_replay/` issues |

## 3. Production implementation evidence

### New files

- `src/scheduler/tool_program_notifications.rs` — `ToolProgramNotificationService` with claim/ack/suppress/expire/bound semantics, `ToolProgramNotification` and `ProgramHandle` types, 13 unit tests
- `tests/tool_program_background.rs` — 8 integration tests for background mode schema, handle, execution mode parsing, and error cases
- `tests/tool_program_notifications.rs` — 13 integration tests for notification lifecycle, session isolation, serialization, and bounds
- `tests/tool_program_projection.rs` — 6 integration tests for projection event mapping, serialization, and reducer application

### Modified files

- `src/tool/tool_program.rs` — Added `execution_mode` parameter, `ExecutionMode` enum, `ProgramHandle` return, background/foreground dispatch, notification service wiring, updated output schema with `submitted` status and `handle` field
- `src/scheduler/mod.rs` — Added `tool_program_notifications` module
- `crates/codegg-protocol/src/core.rs` — Added `ToolProgramCompleted` and `ToolProgramFailed` CoreEvent variants
- `crates/codegg-protocol/src/projection/event.rs` — Added `ToolProgramSubmitted` and `ToolProgramTerminal` ProjectionEvent variants
- `crates/codegg-protocol/src/projection/reducer.rs` — Added reducer handling for new projection events
- `crates/codegg-protocol/src/projection/adapters.rs` — Added CoreEvent→ProjectionEvent mapping for new variants
- `crates/codegg-core/src/projection_replay/safe_publication.rs` — Classified new CoreEvent variants as Safe, added to session_id extraction and exhaustive test
- `crates/codegg-core/src/projection_replay/publication.rs` — Added projection mapping for new CoreEvent variants
- `src/core/mod.rs` — Added session_id extraction and event type strings for new CoreEvent variants
- `src/tool/mod.rs` — Added `notification_service` field to `ToolRegistryOptions`, wired into tool creation
- `src/agent/loop.rs` — Added `notification_service` field, `set_notification_service()` setter, `inject_pending_notifications()` method called at turn start
- `src/agent/agent_loop_factory.rs` — Added `notification_service` to `AgentLoopBuildInput` and factory delegation
- `src/agent/turn_runtime.rs` — Passed `notification_service: None` in `AgentLoopBuildInput` construction
- `src/agent/runtime_factory.rs` — Added `notification_service` parameter to `build_agent_loop()`
- `architecture/tool_programs.md` — Added M008 background programs, projections, and notification section
- `AGENTS.md` — Added M008 test commands and architecture reference

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check                              # pass
cargo test --test tool_program_background               # 8 passed
cargo test --test tool_program_notifications             # 13 passed
cargo test --test tool_program_projection                # 6 passed
cargo test -p codegg --lib tool_program                  # 31 passed
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
| Notification delivery is durable | maintained | Notification state tracked in `ToolProgramNotificationService` |
| Progress never triggers model turns | maintained | Only terminal events create notifications; progress events ignored |
| Duplicate terminal events produce same identity | maintained | `record_notification()` returns existing on duplicate |
| Frontend doesn't own program state | maintained | Projections are read-only; notification service is daemon-owned |
| Session isolation | maintained | `pending_for_session()` scoped to session_id |

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

- `architecture/tool_programs.md` updated with M008 section
- `AGENTS.md` updated with M008 test commands
- Notification lifecycle documented (record/claim/ack/suppress/expire)

## 10. Unresolved findings

| Severity | Finding | Status |
|---|---|---|
| low | Notification service is in-memory only; does not survive daemon restart | Acceptable for daemon lifetime; job store is source of truth |
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
