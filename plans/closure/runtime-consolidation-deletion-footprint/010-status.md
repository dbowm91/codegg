# Runtime Consolidation, Deletion, and Footprint M010 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/010-tui-durable-schedule-identity-and-label-closure.md`
Source subsystem roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`
Controlling addendum: `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md`
Repository baseline reviewed: `78d37ee549ecfc2bb5db10ca54a1a6e21d7ba999`
Implementation commit: `58dd05de — fix tui durable schedule identity and labels`
Final implementation candidate: `58dd05de`

## 1. Executive finding

M010 is strictly closed. The supported TUI schedule workflow now has one coherent
identity contract: `/tasks` and the schedule-created toast display the first eight
characters of the opaque durable ID, while `/task-del` resolves that token against
the active workspace's current `ScheduleList` result before submitting the exact
full ID to the unchanged `ScheduleDelete` protocol.

The TUI also enriches each listed row through the existing `ScheduleGet` record
path, extracting only the durable subagent prompt. A failed detail request leaves
that row usable with a stable kind/state fallback. The missed user-visible
create/list/delete behavior is covered by an in-process daemon regression.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Centralized eight-character display convention | `schedule_display_id()` is used by list rows and schedule-created toasts; exact shorter IDs remain unchanged | Pass |
| Exact full-ID compatibility | `resolve_schedule_id()` checks exact listed IDs before prefix handling; existing daemon full-ID test remains green | Pass |
| Unique short-token deletion | Resolver matches a prefix only at the display length and only when exactly one workspace-visible summary matches | Pass |
| Fail-closed behavior | Resolver tests cover empty, too-short, unknown, and ambiguous input; no delete request is made on resolution failure | Pass |
| Workspace scoping | Delete first requests `ScheduleList { workspace_id: Some(active workspace) }`; resolver consumes only that result | Pass |
| Meaningful labels | `ScheduleGet` returns `ScheduleRecordDto`; `schedule_label()` extracts `job_template.payload.prompt` | Pass |
| Safe enrichment fallback | Detail failure or unsupported payload uses `kind/state`; summary rows are still emitted | Pass |
| Bounded presentation | Row labels remain limited to 30 characters; no private payload fields are rendered | Pass |
| User-visible durable path | `durable_schedule_tui_display_token_resolves_and_deletes_in_workspace` covers create → list → get/label → displayed token → resolve → delete → list absent | Pass |
| Durable authority unchanged | `CoreRequest::ScheduleDelete` still receives a full ID and SQLite still uses `DELETE FROM schedule WHERE id = ?` | Pass |
| Legacy scheduler unchanged/absent | No `BackgroundScheduler` or `BackgroundTask` production definitions were reintroduced; no legacy `Task*` path was changed | Pass |
| No schema/protocol expansion | Only TUI helpers/tests and crate-local test visibility changed; no DTO, migration, scheduler, or public prefix-delete API was added | Pass |

## 3. Production implementation evidence

- `src/tui/commands/tasks.rs` defines the display length once, implements exact-or-unique-prefix resolution, reuses workspace-scoped listing for deletion, and enriches list rows from `ScheduleGet`.
- `src/tui/app/mod.rs` documents that `/task-del` accepts the ID shown by `/tasks` and full schedule IDs.
- `src/core/daemon.rs` contains the mechanism-faithful durable client/daemon regression.
- `src/tui/commands/mod.rs` exposes the pure TUI helpers to in-crate tests only; the re-export is `cfg(test)` and does not widen the runtime API.
- The backend schedule request handlers and `ScheduleStore::delete` were not modified.

## 4. Verification executed

All results below are local results on the final implementation candidate `58dd05de`:

- `cargo fmt --all -- --check` — pass.
- `cargo check -p codegg --locked` — pass.
- `cargo test -p codegg --lib tui::commands::tasks::tests -- --nocapture` — 13 passed.
- `cargo test -p codegg --lib core::daemon::tests::durable_schedule_protocol_supports_create_list_delete -- --nocapture` — 1 passed.
- `cargo test -p codegg --lib core::daemon::tests::durable_schedule_tui_display_token_resolves_and_deletes_in_workspace -- --nocapture` — 1 passed.
- `scripts/verify.sh quick` — pass, including generated-agent validation, core-boundary/sandbox/execution-ownership guards, and workspace all-target locked checking.
- `git diff --check` — pass.

No special hosted run was required by the M010 plan because this is a narrow TUI
correction and the ordinary local quick contract was green. No M006 footprint
measurement was rerun or changed.

## 5. Invariant review

The durable scheduler remains the sole scheduling authority. Schedule IDs remain
opaque strings; the TUI never parses, hashes, normalizes, or manufactures an
authoritative ID. Prefix resolution is presentation-layer behavior over an
already workspace-scoped list and cannot affect non-TUI protocol clients.

Ambiguous, unknown, empty, and too-short inputs fail before `ScheduleDelete`.
Exact full IDs remain supported. No TUI-owned authoritative schedule cache was
introduced.

## 6. Failure and recovery review

Workspace absence, list failure, resolver failure, and core unavailability all
return a completion error without issuing a delete. A failed `ScheduleGet` affects
only its row and falls back to kind/state. The enrichment runs inside the existing
registered asynchronous TUI command, so it does not block rendering.

## 7. Migration and compatibility review

There is no storage migration, DTO change, scheduler change, or public protocol
change. Existing full durable IDs continue to work, and the existing legacy
`Task*` rejection contract remains tested. The change is limited to the supported
TUI compatibility/presentation layer.

## 8. Security review

Resolution is scoped by the active canonical `workspace_id`, never by process CWD,
path heuristics, or global schedule lookup. Ambiguous prefixes fail closed. Label
extraction exposes only the user-visible prompt and bounded presentation text; it
does not dump credentials, provider reasoning, or unrelated job payload metadata.

## 9. Documentation and operations

The `/task-del` usage text now states that the ID shown by `/tasks` is accepted and
that full schedule IDs also work. The M010 implementation plan, closure addendum,
roadmap control surface, and registry are reconciled below.

## 10. Unresolved findings

- Critical: none.
- High: none.
- Medium: none.
- Low: none in M010 scope.
- Deferred: none created by M010. Existing unrelated Provider, Tool Programs,
  development-verification, and runtime-safety dispositions are unchanged.

## 11. Roadmap disposition

The TUI closure addendum is closed. M009 remains historical predecessor evidence
and was not rewritten. The main runtime-consolidation roadmap remains historically
closed, with M010 recorded as the final corrective control point for its current
TUI disposition.

## 12. Registry updates and unblock audit

- M010 moved from active implementation/closure work to recently closed.
- The runtime-consolidation subsystem returned to `closed`; the M010 dependency-ready
  row was removed.
- The blocked-work section and affected dependency graphs were audited. The only
  registered blocked plan is Development Verification and Release M006, whose
  blockers remain Provider M007 and Tool Programs M019; it does not depend on M010.
  Therefore no future plan became dependency-ready and no downstream status was
  changed.
- The implementation plan is marked `implemented`, and the TUI closure addendum is
  marked `closed`.

Final recommendation: **closed**.
