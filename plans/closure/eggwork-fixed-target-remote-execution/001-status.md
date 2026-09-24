# Eggwork Fixed-Target Remote Execution M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/001-fixed-target-finite-job-executor.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Repository baseline reviewed: `e508de52`

Implementation commit:

- `67f8f3d3` — scheduler: fixed-target Eggwork remote executor (M001)

Eggwork dependency (immutable pin, plan §3):

- `eggwork-client` + `eggwork-core`, git `https://github.com/eggstack/eggwork.git`,
  rev `128f808c62f176d414dd18a705773e45f5e2891a`, `default-features = false`
  (no Eggress-routed dialer in M001). `bytes = "1"` added to workspace deps
  for the snapshot pipeline.

## 1. Executive finding

M001 is complete and closed. A CodeGG job can explicitly target one named
Eggwork node; all other jobs default to local execution. The CodeGG
scheduler remains the sole admission/fairness/retry authority and holds its
permit for the whole remote lifetime. One attempt yields at most one
accepted Eggwork execution via a deterministic handle persisted before any
remote side effect. Workspace transfer is bounded and symlink-safe; the
remote never mutates the local workspace in place. Cancellation, progress,
artifacts, and terminal state cross the adapter. Remote failure never falls
back locally or selects another node. Credentials stay daemon/config-owned
(file paths only, key reference redacted from `Debug`). Restart/partition
behavior is conservative: terminal handles reconcile to completions, live
handles are cancelled before a fresh generation. No stop condition (§16)
was triggered; no unresolved medium-or-higher finding remains.

## 2. Requirement-to-evidence matrix (plan §15 acceptance criteria)

| # | Acceptance criterion | Evidence | Result |
|---|---|---|---|
| 1 | Authorized job explicitly targets one named node | `ExecutionTarget::EggworkNode { node_id }` (`crates/codegg-core/src/jobs/mod.rs`); `executor_kind_for_job` target-first (`src/scheduler/executor.rs`); routing matrix §3 | pass |
| 2 | Existing jobs default to local | `ExecutionTarget::default() = Local`; v66 defaults `target_kind='local'`; serde default; protocol conversion defaults | pass |
| 3 | Scheduler is sole admission/fairness/retry authority | `EggworkExecutor` runs inside scheduler dispatch; `JobScheduler` permit spans remote lifetime; `check_scheduler_bypass.py` green | pass |
| 4 | Permit spans whole remote lifetime | `run_remote` holds the scheduler permit across preflight/upload/execute/collect/import; contention e2e test in `tests/eggwork_remote_execution.rs` | pass |
| 5 | One attempt → at most one accepted execution | Deterministic handle `codegg-{sha256(job,attempt)}` gen 1 + fresh lease; `persist_handle` via `set_attempt_remote_handle` before side effects; duplicate-submission test | pass |
| 6 | Workspace transferred safely, never mutated in place | `build_snapshot` (no symlink follow, `..` reject) → `find_missing_blobs`/`upload_blob` → `create_workspace` with `deterministic_workspace_id`; bounds §5; mutation-isolation test | pass |
| 7 | Cancellation/progress/artifacts/terminal state cross adapter | `cancel` on drop/token; `StreamCollector` (200 msgs/2048 chars); `import_artifacts` (16/16 MiB) to `ActualBackend::Eggwork` RunStore records; `map_terminal` | pass |
| 8 | Remote failure never executes locally or picks another node | Typed errors propagate; no fallback path in `EggworkExecutor`; `check_eggwork_target_routing.py` asserts no-spawn/no-fallback; explicit no-local-fallback test | pass |
| 9 | Credentials daemon/config-owned and secret-safe | `EggworkNodeProfile` file-path-only secrets, absolute-path validation, bare-HTTPS endpoint validation, `client_key_path` redacted in `Debug`; secret-negative test | pass |
| 10 | Restart/partition conservative and duplicate-safe | `persisted_handle`/`reconcile_persisted`: terminal→completion, live→cancel+Interrupted; restart-reconcile and lease-expiry tests | pass |
| 11 | Local executors available and unchanged for Local jobs | `executor_kind_for_job` returns existing kinds for `Local`; full suite green (4897 lib + integration) | pass |
| 12 | No unresolved high/medium finding | `scripts/verify.sh full` green; clippy `-D warnings`; all static guards green | pass |

## 3. Local-vs-remote routing matrix

| Job target | `executor_kind_for_job` | Executor | Permit owner |
|---|---|---|---|
| `Local` (default) | existing kind (Build/Test/Shell/…) | existing local executor, unchanged | `JobScheduler` |
| `EggworkNode { node_id }` known | `ExecutorKind::Eggwork` | `EggworkExecutor` | `JobScheduler` (spans remote lifetime) |
| `EggworkNode { node_id }` unknown | `ExecutorKind::Eggwork` → typed config error before side effects | none (fail-closed) | n/a |
| Remote failure at any phase | error propagates | none (no fallback, no re-target) | permit released |

## 4. Durable target/provenance schema evidence

- Migration v66 (`crates/codegg-core/src/session/schema.rs::migrate_v66`):
  `job.target_kind TEXT NOT NULL DEFAULT 'local'`,
  `job.target_node_id TEXT`,
  `job_attempt.remote_handle_json TEXT`,
  `idx_job_target_node`, `STORAGE_LAYOUT_VERSION = 66`.
- ADD COLUMNs use `add_column_ignore_duplicate` (house idempotency helper);
  index uses `IF NOT EXISTS`.
- `RemoteExecutionHandle { schema_version = 1, node_id, execution_id, generation, lease_id }`
  round-trips through `JobStore::set_attempt_remote_handle` and
  `JobSummary.target_node_id`.

## 5. Workspace transfer bounds

4096 entries / 256 MiB total / 64 MiB per file / 128 depth; symlinks never
followed; `..` escapes rejected; missing-blobs-only upload; deterministic
workspace id; per-file SHA-256 verified against `BlobDigest` on upload.

## 6. Focused and full verification

- `tests/eggwork_remote_execution.rs`: 20 tests via `ScriptedClient`/
  `ScriptedFactory` seam (routing, SQLite round-trip, pre-flight, argv
  fixture, idempotency, restart reconcile/cancel-live, cancellation, lease
  renewal, cwd/symlink, artifact import, scheduler e2e with `eggwork`
  provenance). 20/20 pass.
- `src/scheduler/eggwork.rs` unit tests: 6/6 pass.
- `scripts/verify.sh quick`: pass. `scripts/verify.sh full`: pass
  (clippy `-D warnings`, workspace tests, `server,plugins,lsp-test-support`
  feature tests). One transient full-suite-only failure in
  `tool::lsp_preview_apply::tests::fresh_tool_registry_expires_prior_preview_id`
  (file untouched by this change-set; 4/4 green in isolation) did not recur
  on the gating re-run.
- Static guards: `check_eggwork_target_routing.py` (new, wired into
  `verify.sh`), `check_execution_ownership.py`, `check_scheduler_bypass.py`,
  `check_daemon_cwd_usage.py`, core-boundary — all green.

## 7. Residual findings and M002 disposition

- `Test` payloads stay `InvalidPayload` on the Eggwork path (deferred to
  M002 per plan); shell argv fixtures cover Build/Lint/Format/ManagedProcess/Shell.
- Live-model (non-scripted) Eggwork qualification was out of scope; the
  `EggworkNodeClient` trait seam exists for it.
- M002 (broader executor coverage / placement) is unblocked: durable target,
  handle persistence, and the routing guard are in place.
