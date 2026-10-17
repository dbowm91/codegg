# Project Picker Rework Milestone 003 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/project-picker-rework/003-lifecycle-and-confirmed-destroy.md`
Source subsystem roadmap: `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
Repository baseline reviewed: `7d092c01`
Implementation commit: `eecd75e3` — atomic project lifecycle and confirmed destroy

## 1. Executive finding

M003 is closed. Active projects can be archived and restored through the
picker. Destroy is available only for archived projects and requires a
confirmation that names the project, exact session count, and permanent
session deletion. The catalog cascade is transactional and refuses an
active project or a stale confirmation count. Successful deletion cancels
turns and pending permission/question waiters for affected sessions,
closes their tabs, publishes a scoped event, and records actor and count
in audit metadata. Local focused tests and `scripts/verify.sh quick`
passed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Finding |
|---|---|---|
| Archive/restore retain sessions and move picker rows through lifecycle state | Project catalog lifecycle tests; TUI project suite | Satisfied |
| Destroy only an archived project after explicit confirmation | Catalog refuses active projects; picker uses a separate confirm phase and sends no request on dismiss; confirmation render test checks permanence copy and count | Satisfied |
| Atomic project/session/binding cascade with expected count | `cargo test -p codegg-core --test project_catalog` — 20 passed, including cascade, stale count, active refusal, and injected-trigger rollback | Satisfied |
| Authorization denial after membership revocation | `cargo test -p codegg --test identity_m003_daemon_authorization membership_revocation_takes_effect_at_boundary` — 1 passed, including destroy denial | Satisfied |
| Audited lifecycle actor and session counts | `cargo test -p codegg --test identity_m005_audit_instrumentation project_destroy_audit_carries_actor_and_confirmed_session_count` — 1 passed | Satisfied |
| Deleted sessions fail closed and do not leave approval/question waiters parked | Bus tests verify session-scoped permission/question sender cleanup; daemon cancels turns and removes runtimes | Satisfied |
| Focused picker interaction and modifier contract | `cargo test -p codegg --lib tui::` — 1,707 passed; `cargo test -p codegg --lib project` — 494 passed; `cargo test -p codegg --test modifier_chords` — 16 passed | Satisfied |
| Repository boundaries and workspace sanity | `scripts/verify.sh quick` passed, including `cargo check --workspace --all-targets --locked` | Satisfied |

## 3. Production implementation evidence

- Added `ProjectCatalog::delete_project`, which verifies archival and the
  caller's confirmed session count inside the catalog operation, then
  deletes sessions and project bindings in one SQL transaction.
- Added additive core protocol request/response/event, a `project_delete`
  authorization operation, the HTTP DELETE route, and post-success audit
  metadata for archive, restore, and destroy.
- Added picker `d` archive / archived destroy, `a` restore, and an explicit
  dismissible confirmation phase. Successful destroy removes catalog
  state, closes affected tabs, and refreshes projects.
- Project deletion cancels active runtime turns and drops session-scoped
  permission/question senders so pending waits resolve closed rather than
  retaining a dead session.
- Updated `docs/tui.md` and the project, protocol, and authorization
  architecture documents. No migration was needed.

## 4. Verification executed

Commands ran locally on implementation commit `eecd75e3`. The active Rust
target is x86_64 while the default pkg-config paths select arm64 MacPorts
libraries, so Rust checks used
`PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig CARGO_BUILD_JOBS=2`.

- `cargo test -p codegg-core --test project_catalog` — 20 passed.
- `cargo test -p codegg-core bus::tests` — 4 passed.
- `cargo test -p codegg --lib tui::` — 1,707 passed.
- `cargo test -p codegg --lib project` — 494 passed.
- `cargo test -p codegg --test modifier_chords` — 16 passed.
- `cargo test -p codegg --test identity_m003_daemon_authorization membership_revocation_takes_effect_at_boundary` — 1 passed.
- `cargo test -p codegg --test identity_m005_audit_instrumentation project_destroy_audit_carries_actor_and_confirmed_session_count` — 1 passed.
- `cargo check -p codegg --lib --features server` — passed.
- `scripts/verify.sh quick` — passed, including format, static guards, and the locked all-target workspace check.
- `git diff --check` — passed before the implementation commit.

Hosted CI, a manual TUI walkthrough, and reinstall/daemon-restart checks
were not run. They are recorded as operational evidence limits, not as
local acceptance failures.

## 5. Failure, recovery, and compatibility review

The catalog operation rejects stale counts before mutating and runs all
deletions in one transaction. An injected trigger failure proves rollback
preserves project/session state. A repeated deletion sees a missing
project and returns the existing not-found error path. The protocol
change is additive; no storage layout or migration version changed.

## 6. Security and authority review

Destroy uses a dedicated `project_delete` operation with project
configuration authority. The server route checks path/body identity and
authorization. The daemon audit records the trusted principal and
confirmed count. Event publication carries the deleted project scope.
The TUI asks for the exact count and performs no request when the dialog
is dismissed. No daemon cwd mutation or new execution authority was
introduced.

## 7. Unresolved findings

None within M003 scope. Hosted CI and interactive restart evidence were
not collected locally.

## 8. Downstream dependency audit

- M004 rename and repath was already ready after M002. It remains
  eligible and is activated as the next milestone in the requested
  sequence.
- M005 remains blocked on strict closure of M002–M004. M003 closure
  satisfies one of its gates; M004 is still outstanding.
- No other registered project-picker plan became newly eligible.

## 9. Registry updates

- M003 moved to closed and this record was added to recently closed
  work.
- M004 moved from ready to active for sequential implementation.
- M005 remains blocked on M002–M004.
- The project-picker roadmap remains active until M005 closes.
