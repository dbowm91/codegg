# Execution ownership inventory

CodeGG tracks process launches, work dispatch, test-runner entry points, and
durable-job creation so contributors can see which component owns execution.
The machine-readable inventory is `docs/execution-ownership.toml`; the static
guard is `scripts/check_execution_ownership.py`. For recognized source
patterns, the guard accepts either a whole-file manifest entry or an inline
`// execution-ownership: <owner>` annotation on the matched line or the line
above it. It is a source check, not proof of runtime behavior. The runtime
scheduler assertion is `tests/scheduler_jobs/scheduler_authority_matrix.inc`.

## Owner classes

| Owner | Meaning |
|---|---|
| `scheduler` | Production daemon path that admits heavy work through `JobSubmissionService`. |
| `interactive` | Long-lived, user-controlled PTY, REPL, or editor session. |
| `standalone_compat` | Explicit `--standalone`, `--stdio`, or test-harness path outside the daemon singleton guarantee. |
| `definition_or_adapter` | Defines or adapts a canonical subsystem but does not invoke it independently. |
| `deferred_domain_executor` | Compatibility path whose typed subsystem is planned for scheduler integration. |
| `test_only` | Test fixture or test-only source. |
| `forbidden_bypass` | Must be fixed; the static guard rejects this classification. |

## Adding or changing an execution site

1. Prefer a `[[site]]` entry in `docs/execution-ownership.toml` with `path`,
   `owner`, and a short `reason`. Add `entrypoint` when it helps identify the
   canonical submission boundary. Use an inline annotation only when the
   classification belongs to one matched call site rather than the whole file.
2. Run `python3 scripts/check_execution_ownership.py`. The guard rejects
   unclassified recognized patterns and forbidden owners. Its `--self-test`
   checks that representative output-collection and argument-parsing bypasses
   remain rejected.
3. Do not record a `forbidden_bypass` as an accepted exception. Route or remove
   the bypass first.

## Process lifecycle

Finite local process execution is owned by `ManagedProcessService` in
`src/managed_process.rs`. Scheduler executors admit durable jobs; the service
runs each accepted attempt and owns its process lifecycle, including typed
arguments, working directory and environment, cancellation, timeouts, bounded
output, sandbox setup, cleanup, and reaping. Long-lived interactive sessions
use a separate owner and are not finite job submissions. See
`architecture/process-tool-execution-ownership.md` for the process contract and
exception inventory.

Eggwork-targeted jobs execute on the selected remote node and do not start a
local child process. A remote failure does not fall back to local execution.
The routing guard is `scripts/check_eggwork_target_routing.py`.

## Source verification

Previously checked 2026-10-23 against the source tree and guard. Rechecked
2026-11-15 against `docs/execution-ownership.toml`,
`scripts/check_execution_ownership.py`, `src/managed_process.rs`,
`src/scheduler/executors.rs`, `src/interactive_process.rs`,
`src/scheduler/eggwork.rs`, `scripts/check_eggwork_target_routing.py`, and
`tests/scheduler_jobs/scheduler_authority_matrix.inc`.

- The guard's recognized patterns, skipped scopes, manifest and inline
  classifications, and negative self-tests are defined in
  `scripts/check_execution_ownership.py`. The earlier wording implied that a
  manifest entry was the only accepted classification; inline annotations are
  also accepted.
- Finite process execution and scheduler admission are separated between
  `src/managed_process.rs` and `src/scheduler/executors.rs`.
- Eggwork routing is remote and does not fall back to local process execution,
  as implemented in `src/scheduler/eggwork.rs` and checked by
  `scripts/check_eggwork_target_routing.py`.
