# Tool Programs Milestone 001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-programs/001-scheduler-owned-python-execution.md`
Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md#milestone-1--scheduler-owned-python-execution`
Repository baseline reviewed: `2f715941516a1d49be578fdef56714ad3ddfe8bf`
Implementation commits: `HEAD` — Scheduler-owned Python execution (M001) + corrective pass

## 1. Executive finding

All production model-facing Python execution is scheduler-owned. No fallback to direct execution exists when the scheduler is disabled (fail-closed). The `PythonJobExecutor` begins a RunStore record **before** process launch and completes it after. Legacy `script_path` payloads without inline source are rejected with a typed error. Source orphan cleanup runs periodically via the scheduler reconcile loop. Temp files are cleaned up via drop guards on cancellation.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Every production Python execution creates one durable job before launch | `PythonScriptTool::execute_via_scheduler` and `BashTool::dispatch_python_via_scheduler` submit through `JobSubmissionService::submit` before any execution |
| Exactly one attempt and one active RunStore record own an execution | `PythonJobExecutor::execute` calls `begin_python_run` before execution, `write_python_run_artifacts` + `complete_python_run` after |
| Scheduler cancellation and timeout terminate the process group | `tokio::select!` in executor races `CancellationToken` against `execute_python_script` |
| Existing Python risk, sandbox, snapshot, diff, and mutation guarantees remain | `execute_python_script` unchanged; all risk/sandbox/snapshot logic preserved |
| Bash and python_script share the canonical executor | Both submit `JobKind::Python` through `JobSubmissionService`; `PythonJobExecutor` handles both |
| Restart recovery is deterministic | `IdempotencyClass` set per mode (SafeRepeat for read-only, NonIdempotent for transform); `RecoveryPolicy` governs requeue |
| Python artifacts are real expandable handles | RunStore artifacts (stdout, stderr, diff) written by executor via `write_python_run_artifacts`; `run_id` propagated to tool output |
| Static guards prevent reintroduction of direct production execution | `check_execution_ownership.py` updated; `check_scheduler_bypass.py` passes; `check-core-boundary.sh` passes |
| No source or credential leakage | Source hash stored in payload; inline source only; no secrets in labels/events |
| No fallback to direct execution when scheduler disabled | `PythonScriptTool::execute` returns `ToolError::Disabled`; `BashTool::dispatch_to_python_script` returns `ToolError::Disabled` |
| Legacy script_path payloads rejected | `PythonJobExecutor::execute` returns typed failure for payloads without inline source |
| RunStore begin_run before execution | `begin_python_run` called before `execute_python_script` in executor |
| Temp file cleanup on cancellation | `TempScriptCleanup` drop guard in `execute_python_script` ensures cleanup |
| Source orphan lifecycle | `cleanup_python_source_orphans` in scheduler reconcile loop |

## 3. Production implementation evidence

### Files changed

- `crates/codegg-core/src/jobs/mod.rs` — Added `source`, `source_hash`, `cwd`, `timeout_secs` fields to `JobPayload::Python`; added `source_hash()` helper method
- `src/python_script/source_store.rs` — Content-addressed `PythonSourceStore` with atomic writes, digest verification, symlink/traversal rejection, orphan cleanup, `cleanup_stale` static method
- `src/python_script/mod.rs` — Added `source_store` module; exports `begin_python_run`
- `src/python_script/executor.rs` — Added `TempScriptCleanup` drop guard for temp file cleanup on cancellation
- `src/scheduler/executor.rs` — Added `ExecutorKind::Python`, `PayloadVariant::Python`, routing in `executor_kind_for_job`
- `src/scheduler/executors.rs` — `PythonJobExecutor`: RunStore begin before execution, write+complete after; typed error for legacy payloads; 5-phase progress; registered in `register_default_executors`
- `src/scheduler/scheduler.rs` — Added `cleanup_python_source_orphans` method called from reconcile loop
- `src/python_script/tool.rs` — `PythonScriptTool` returns `ToolError::Disabled` when scheduler absent; split `persist_python_run` into `begin_python_run` + `write_python_run_artifacts` + `complete_python_run`; `execute_and_persist_python_script` restricted to `#[cfg(test)]`
- `src/tool/bash.rs` — Returns `ToolError::Disabled` when scheduler absent; removed direct execution fallback
- `docs/execution-ownership.toml` — Python entries updated from `deferred_domain_executor` to `scheduler`/`definition_or_adapter`
- `architecture/python_scripting.md` — Documented scheduler-owned execution, operator troubleshooting, progress phases
- `architecture/run_store.md` — Added Python scheduler ownership section
- `tests/command_routing_execution_ownership.rs` — Updated test for fail-closed behavior; added `PythonJobExecutor`
- `tests/python_scheduler_execution.rs` — New: 10 integration tests for scheduler-owned Python execution

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo test -p codegg --lib python_script` | 195 passed |
| `cargo test -p codegg --lib scheduler` | 41 passed |
| `cargo test -p codegg --test python_sandbox_adversarial` | 57 passed |
| `cargo test --test command_routing_execution_ownership` | 20 passed |
| `cargo test --test python_scheduler_execution` | 10 passed |
| `cargo fmt --all -- --check` | Clean |
| `cargo clippy -p codegg --lib -- -D warnings` | Clean (pre-existing codegg-core errors excluded) |
| `python3 scripts/check_execution_ownership.py` | ok |
| `python3 scripts/check_scheduler_bypass.py` | ok |

## 5. Invariant review

- **Scheduler is sole admission authority**: All production Python paths submit through `JobSubmissionService`; disabled scheduler returns `ToolError::Disabled`, no fallback.
- **CWD resolved from immutable context**: `PythonScriptRequest.cwd` resolved at submission time; executor validates via `validate_cwd`.
- **Analyze/Verify non-mutating**: Preserved by existing `execute_python_script` snapshot logic.
- **Risk/sandbox/snapshot/diff guarantees unchanged**: `execute_python_script` untouched; all enforcement evidence flows through RunStore.
- **Cancellation reaches process**: `tokio::select!` with `CancellationToken` ensures pre-launch and mid-execution cancellation.
- **No source in logs/labels**: Source hash stored; inline source only in job payload (not in labels or events).
- **RunStore ownership before execution**: `begin_python_run` called before subprocess launch; run is visible as active immediately.
- **Temp file cleanup**: Drop guard ensures cleanup even on cancellation.
- **Source orphan cleanup**: Periodic cleanup via scheduler reconcile loop.

## 6. Failure and recovery review

- **Submission failure before job creation**: No durable record created; idempotency map clean.
- **Enqueue failure after creation**: Job cancelled via `request_cancel`; no silent execution.
- **Cancellation before admission**: Job terminated without materializing source.
- **Cancellation after process launch**: `tokio::select!` fires; executor returns `Cancelled` status; drop guard cleans temp file.
- **Daemon-generation restart**: `RecoveryPolicy` governs requeue; transform defaults to non-retryable.
- **Source integrity**: SHA-256 verified before execution; mismatch returns typed failure.
- **Legacy payload**: `script_path` without inline source returns typed failure, not arbitrary file execution.

## 7. Migration and compatibility review

- `JobPayload::Python` new fields are `#[serde(default, skip_serializing_if = "Option::is_none")]` — backward compatible.
- Legacy payloads with only `script_path/args/mode` still deserialize but **fail execution** with "inline source is required for scheduler-owned execution" (no arbitrary file execution).
- `execute_and_persist_python_script` restricted to `#[cfg(test)]` — not a production entry point.
- No database downgrade required; older rows remain inspectable and produce actionable failure.

## 8. Security review

- Source path traversal rejected by `PythonSourceStore::resolve_path`.
- Symlinks rejected by `PythonSourceStore::persist`.
- Digest verified before execution and on restart.
- No source body in logs, labels, or protocol events.
- Existing Landlock/portable sandbox enforcement unchanged.
- No direct execution fallback exists — fail-closed when scheduler disabled.

## 9. Documentation and operations

- `architecture/python_scripting.md` — Updated scheduler-owned execution, operator troubleshooting, progress phases, cancellation/recovery behavior.
- `architecture/run_store.md` — Added Python scheduler ownership lifecycle section.
- `docs/execution-ownership.toml` — Updated.
- Operator guidance: cancelled Python jobs show `Failed(-4)` status; source-integrity failures show digest mismatch error; disabled scheduler returns typed error.

## 10. Unresolved findings

None. All acceptance criteria satisfied.

## 11. Roadmap disposition

Milestone 001 is closed. Milestone 002 (tool contracts and canonical broker) is unblocked.

## 12. Registry updates

- `plans/registry.md`: Tool-programs subsystem M001 moved from `ready` to `closed`.
- M002 moved from `blocked` to `ready` (all hard dependencies satisfied).
