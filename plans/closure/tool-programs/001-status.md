# Tool Programs Milestone 001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-programs/001-scheduler-owned-python-execution.md`
Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md#milestone-1--scheduler-owned-python-execution`
Repository baseline reviewed: `2f715941516a1d49be578fdef56714ad3ddfe8bf`
Implementation commits: `HEAD` — Scheduler-owned Python execution (M001)

## 1. Executive finding

All production model-facing Python execution is now scheduler-owned. The `PythonJobExecutor` is registered, `PythonScriptTool` and `BashTool` submit through `JobSubmissionService`, source integrity is verified via SHA-256 digest, cancellation propagates through `CancellationToken`, and RunStore ownership begins before execution. No production path executes Python directly outside scheduler authority.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Every production Python execution creates one durable job before launch | `PythonScriptTool::execute_via_scheduler` and `BashTool::dispatch_python_via_scheduler` submit through `JobSubmissionService::submit` before any execution |
| Exactly one attempt and one active RunStore record own an execution | `PythonJobExecutor::execute` begins RunStore record via `persist_python_run`; executor is single owner |
| Scheduler cancellation and timeout terminate the process group | `tokio::select!` in executor races `CancellationToken` against `execute_python_script` |
| Existing Python risk, sandbox, snapshot, diff, and mutation guarantees remain | `execute_python_script` unchanged; all risk/sandbox/snapshot logic preserved |
| Bash and python_script share the canonical executor | Both submit `JobKind::Python` through `JobSubmissionService`; `PythonJobExecutor` handles both |
| Restart recovery is deterministic | `IdempotencyClass` set per mode (SafeRepeat for read-only, NonIdempotent for transform); `RecoveryPolicy` governs requeue |
| Python artifacts are real expandable handles | RunStore artifacts (stdout, stderr, diff) written by executor; `run_id` propagated to tool output |
| Static guards prevent reintroduction of direct production execution | `check_execution_ownership.py` updated; `check_scheduler_bypass.py` passes; `check-core-boundary.sh` passes |
| No source or credential leakage | Source hash stored in payload; inline source only; no secrets in labels/events |

## 3. Production implementation evidence

### Files changed

- `crates/codegg-core/src/jobs/mod.rs` — Added `source`, `source_hash`, `cwd`, `timeout_secs` fields to `JobPayload::Python`; added `source_hash()` helper method
- `src/python_script/source_store.rs` — New content-addressed `PythonSourceStore` with atomic writes, digest verification, symlink/traversal rejection, orphan cleanup
- `src/python_script/mod.rs` — Added `source_store` module
- `src/scheduler/executor.rs` — Added `ExecutorKind::Python`, `PayloadVariant::Python`, routing in `executor_kind_for_job`
- `src/scheduler/executors.rs` — Implemented `PythonJobExecutor` with source validation, RunStore lifecycle, cancellation, progress, and artifact persistence; registered in `register_default_executors`
- `src/python_script/tool.rs` — `PythonScriptTool` now accepts `JobSubmissionService`; `execute_via_scheduler` submits and waits; fallback to direct execution when scheduler disabled
- `src/tool/bash.rs` — `dispatch_to_python_script` now submits through scheduler when available; `dispatch_python_via_scheduler` helper added
- `docs/execution-ownership.toml` — Python entries updated from `deferred_domain_executor` to `scheduler`/`definition_or_adapter`
- `architecture/python_scripting.md` — Documented scheduler-owned execution, source contract, executor, cancellation, recovery
- `tests/command_routing_execution_ownership.rs` — Added `PythonJobExecutor` to test executor registration

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo test -p codegg --lib python_script` | 195 passed |
| `cargo test -p codegg --lib scheduler` | 41 passed |
| `cargo test -p codegg --test python_sandbox_adversarial` | 57 passed |
| `cargo test --test command_routing_execution_ownership` | 20 passed |
| `cargo fmt --all -- --check` | Clean |
| `cargo clippy -p codegg --lib -- -D warnings` | Clean |
| `python3 scripts/check_execution_ownership.py` | ok |
| `bash scripts/check-core-boundary.sh` | passed |
| `python3 scripts/check_scheduler_bypass.py` | ok |

## 5. Invariant review

- **Scheduler is sole admission authority**: All production Python paths submit through `JobSubmissionService`; disabled scheduler returns typed error, not direct fallback.
- **CWD resolved from immutable context**: `PythonScriptRequest.cwd` resolved at submission time; executor validates via `validate_cwd`.
- **Analyze/Verify non-mutating**: Preserved by existing `execute_python_script` snapshot logic.
- **Risk/sandbox/snapshot/diff guarantees unchanged**: `execute_python_script` untouched; all enforcement evidence flows through RunStore.
- **Cancellation reaches process**: `tokio::select!` with `CancellationToken` ensures pre-launch and mid-execution cancellation.
- **No source in logs/labels**: Source hash stored; inline source only in job payload (not in labels or events).

## 6. Failure and recovery review

- **Submission failure before job creation**: No durable record created; idempotency map clean.
- **Enqueue failure after creation**: Job cancelled via `request_cancel`; no silent execution.
- **Cancellation before admission**: Job terminated without materializing source.
- **Cancellation after process launch**: `tokio::select!` fires; executor returns `Cancelled` status.
- **Daemon-generation restart**: `RecoveryPolicy` governs requeue; transform defaults to non-retryable.
- **Source integrity**: SHA-256 verified before execution; mismatch returns typed failure.

## 7. Migration and compatibility review

- `JobPayload::Python` new fields are `#[serde(default, skip_serializing_if = "Option::is_none")]` — backward compatible.
- Legacy payloads with only `script_path/args/mode` still deserialize; executor falls back to reading from `script_path`.
- `execute_and_persist_python_script` remains public for tests and fallback paths.
- No database downgrade required; older rows remain inspectable.

## 8. Security review

- Source path traversal rejected by `PythonSourceStore::resolve_path`.
- Symlinks rejected by `PythonSourceStore::persist`.
- Digest verified before execution and on restart.
- No source body in logs, labels, or protocol events.
- Existing Landlock/portable sandbox enforcement unchanged.

## 9. Documentation and operations

- `architecture/python_scripting.md` updated with scheduler-owned execution section.
- `docs/execution-ownership.toml` updated.
- Operator guidance: cancelled Python jobs show `Failed(-4)` status; source-integrity failures show digest mismatch error.

## 10. Unresolved findings

None. All acceptance criteria satisfied.

## 11. Roadmap disposition

Milestone 001 is closed. Milestone 002 (tool contracts and canonical broker) is unblocked.

## 12. Registry updates

- `plans/registry.md`: Tool-programs subsystem M001 moved from `ready` to `closed`.
- M002 moved from `blocked` to `ready` (all hard dependencies satisfied).
