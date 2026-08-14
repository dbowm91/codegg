# Runtime Consolidation, Deletion, and Footprint M009 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/009-final-corrective-runtime-consolidation-closure.md`
Source corrective addendum: `plans/subsystems/runtime-consolidation-deletion-footprint-corrective-closure-addendum.md`
Source subsystem roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Exact baseline: `f1e4c16f1bfe16cad57fb6fc290d48ab03974072`

Accepted final production candidate: `c8c31d909310131ca4b1cc38c725e0163f86a47d`

Implementation commit: `c8c31d90` — restore durable TUI schedules and extract
provider turns

Hosted evidence: [CI run 31724978736](https://github.com/dbowm91/codegg/actions/runs/31724978736), [verify job 94530985774](https://github.com/dbowm91/codegg/actions/runs/31724978736/job/94530985774), green on the exact candidate SHA.

## 1. Executive finding

M009 is strictly closed. It corrected the two audited production gaps, reran
the final-tree M006 evidence, reconciled M003/M006/M007/M008 planning state,
completed the broad local verification contract, and obtained one ordinary
hosted `CI / verify` result on the exact accepted candidate. The roadmap is
therefore closed; no documentation-only closure was used.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Active TUI schedule flow uses durable API | `src/tui/commands/tasks.rs`; TUI unit tests | pass |
| Durable create/list/delete protocol works | `core::daemon::tests::durable_schedule_protocol_supports_create_list_delete` | pass |
| Legacy scheduler remains deleted | `BackgroundScheduler`, `BackgroundTask`, `src/agent/task.rs`, and independent timer/persistence path remain absent | pass |
| No UUID/u64 bridge returns | no active TUI numeric parsing; `Task*` requests only explicit rejection boundary | pass |
| Provider-turn body has physical ownership | retry/stream/normalization/usage body is in `src/agent/provider_turn.rs`; loop-owned implementation removed | pass |
| Provider semantics preserved | loop/provider focused tests and agent-loop harness | pass |
| M006 final-tree evidence complete | `plans/closure/runtime-consolidation-deletion-footprint/006-status.md` | pass |
| M007 strict integration evidence complete | `plans/closure/runtime-consolidation-deletion-footprint/007-status.md` | pass |
| Broad local verification complete | capped workspace test run; all suites green | pass |
| Exact hosted verification complete | run `31724978736`, job `94530985774` | pass |
| No in-scope critical/high/medium finding remains | findings disposition below | pass |

## 3. Findings A–E disposition

| Finding | Disposition |
|---|---|
| A — active TUI task capability regressed after M001 | Corrected by routing schedule/list/delete to durable `Schedule*` requests with canonical workspace/session context. |
| B — provider-turn extraction remained façade-only | Corrected by moving the real retry, stream, timeout, normalized-event, usage, and error implementation into `ProviderTurnAdapter`. |
| C — M006 lacked final-tree strict evidence | Corrected and closed in `006-status.md` with final feature/dependency/release measurements. |
| D — M007 advanced before hard predecessor/evidence completion | Corrected and closed in `007-status.md` only after M006, broad local verification, and exact hosted CI were complete. |
| E — planning/documentation state drift | Corrected across M003, M006, M007, M008, M009, the addendum, roadmap, registry, and architecture ownership text. |

## 4. TUI durable-schedule evidence

The TUI handlers now use the active session's `workspace_id` and `session_id`.
Create constructs the existing durable interval `ScheduleCreateDto` and
subagent `JobTemplate`; list sends `ScheduleList` scoped to the workspace and
projects `ScheduleSummaryDto`; delete sends `ScheduleDelete` with an opaque
schedule ID. Create/list/delete response handling is explicit and bounded.
The focused tests prove the DTO contract and durable summary projection, while
the daemon protocol test proves create/list/delete against the durable store.

The retained `CoreRequest::TaskSchedule`, `TaskList`, and `TaskDelete` paths
remain deterministic unsupported responses for old external callers. They do
not reconstruct a legacy scheduler or persistence path.

## 5. Legacy deletion and provider ownership evidence

Source inspection on the final candidate confirms that `BackgroundScheduler`,
`BackgroundTask`, `src/agent/task.rs`, the independent timer loop, and the old
task persistence interpretation remain absent. No active TUI handler contains
the legacy `Task*` requests or UUID-to-`u64` parsing.

`provider_turn.rs` owns the concrete provider-turn body: retry count/backoff,
cancellation, request timeout and stall timeout, provider stream consumption,
normalized event publication, usage-store insertion, unaccounted-token
accounting, and provider error conversion. `loop.rs` retains orchestration and
no longer contains `stream_with_retry_impl` or `stream_once`.

## 6. Focused and broad verification

Focused commands passed:

```text
cargo fmt --all -- --check
cargo check -p codegg --lib --locked
cargo test -p codegg --lib tui::commands::tasks::tests -- --nocapture
cargo test -p codegg --lib core::daemon::tests::durable_schedule_protocol_supports_create_list_delete -- --nocapture
cargo test -p codegg-core jobs::schedule -- --nocapture
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
scripts/verify.sh quick
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check -p codegg --locked --features server,plugins,lsp-test-support
git diff --check
```

The required broad command also passed:

```text
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
```

It completed successfully across the workspace, integration suites, and doc
tests. Provider credential warnings in tests were expected for unset local
credentials and did not fail a test.

## 7. M006 final measurements

The isolated release artifacts on Rust/Cargo `1.97.1` and `aarch64-apple-darwin`
were:

| Surface | Command | Size |
|---|---|---:|
| Default | `cargo build --release --locked --bin codegg` | 54,347,840 bytes |
| Production features | `cargo build --release --locked --bin codegg --features server,plugins,lsp-test-support` | 63,566,624 bytes |

Both used isolated `CARGO_TARGET_DIR` values. Feature and duplicate trees were
reviewed; no safe dependency/feature/profile/topology reduction was justified.

## 8. Invariant, storage, protocol, migration, and security review

No schema migration is required. The TUI reuses existing durable schedule/job
tables and stores, and no legacy in-memory task persistence returns. The
durable protocol remains the single source of truth; schedule IDs remain
opaque. Existing workspace/session authority, scheduler admission, permission
and Tool Broker checks, cancellation/retry behavior, provider wire formats,
private-reasoning projection, path policy, credential handling, sandbox, and
execution-ownership boundaries are unchanged.

## 9. Planning and future-plan reconciliation

M003 and its corrective physical-extraction plan are marked closed/implemented
with the actual implementation commit. M006 and M007 now have strict closure
records with final-tree and hosted evidence. M008's stale handoff state is
marked implemented under M009, and the M009 implementation plan points to this
record. The roadmap and registry agree that runtime consolidation is closed.

The registry audit found no unrelated registered future plan newly unblocked by
this closure. Tool Programs M019 remains ready for its independent strict
review. Development Verification and Release M006 remains blocked on Provider
M007 and Tool Programs M019, so its status was intentionally not changed.

## 10. Unresolved findings by severity

| Severity | Findings |
|---|---|
| Critical | None |
| High | None |
| Medium | None |
| Low | None requiring follow-up in this scope |
| Deferred | Independent Provider M007, Tool Programs M019, and DVR M006 workstreams; retained under their own plans |

## 11. Final recommendation

Closed. The runtime-consolidation corrective addendum and its source roadmap
may be marked closed after this record is committed with the reconciled
registry and roadmap. No follow-on runtime-consolidation milestone is created.
