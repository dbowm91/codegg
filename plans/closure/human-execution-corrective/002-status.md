# Human Execution Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/human-execution-corrective/002-workspace-owned-human-shell-dispatch.md`

Source subsystem roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c002--workspace-owned-finite-shell-dispatch`

Repository baseline reviewed: `2c2d6f1308c11738ca185163b3405e90d8eed789`

Implementation commits:

- `0c37cc9c8a5e3cd62075bd9cad9a487ea235c239` — daemon-owned finite human-shell protocol, admission, streaming, cancellation, TUI routing, and documentation.
- `5448413a391735fb5d3fa7f1be72f08387c306f5` — Clippy corrections for compact scope-validation errors and bounded run pruning.

Hosted qualification on implementation SHA `5448413a391735fb5d3fa7f1be72f08387c306f5`:

- [Root CI run 38263761126](https://github.com/dbowm91/codegg/actions/runs/38263761126) — success; all guards, formatting, Workspace Clippy, and 12,789 tests passed (5 skipped).
- [Desktop E2E run 38263761188](https://github.com/dbowm91/codegg/actions/runs/38263761188) — success, including the built-app WebDriver trajectory.

## 1. Executive finding

C002 is complete. Bang commands now run through the authorized workspace's daemon execution owner. The frontend cannot select a host shell, working directory, environment, or remote node in the request, and has no direct-process fallback. Local daemon, standalone core, and authenticated remote-attached TUI topologies use the same finite-run service. A workspace bound to a remote execution node receives an explicit unsupported-target response.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Execution follows daemon-owned workspace authority, not frontend cwd or environment | `src/core/daemon_human_shell.rs`; typed DTO contains no client cwd/environment fields; daemon-handler test `poolless_standalone_human_shell_uses_registered_workspace` | pass | Workspace root is resolved from the daemon registry; pooled runs also require resolved project/workspace and optional session bindings. |
| Foreign, archived, mismatched, or remote-node workspaces fail closed | Daemon scope validation and remote-node disposition in `src/core/daemon_human_shell.rs`; authorization matrix and hosted execution-ownership guards | pass | Remote-node federation remains outside this milestone. |
| TUI submits human intent through the negotiated daemon capability, with no local spawn fallback | `src/tui/commands/shell.rs`, `src/tui/runtime/command_dispatch.rs`; capability negotiation and typed run operations | pass | Older peers return an unsupported-capability error. Captured workspace/session scope remains fixed across tab changes. |
| Runs are finite, bounded, cancellable, and scheduler-admitted | `src/shell/daemon.rs`; 5 focused `shell::daemon` tests | pass | Uses the shared process admission controller, timeout, output/chunk bounds, cursor/truncation behavior, and cancellation. |
| Duplicate submission is safe and scoped output cannot cross principals/workspaces | `two_principals_and_workspaces_keep_cwd_and_output_isolated`, `duplicate_admission_is_idempotent_and_changed_content_conflicts` | pass | Reuse of a submission ID with changed content or scope conflicts. |
| Restart and disconnect semantics are explicit | `cancellation_terminates_the_managed_process_and_restart_loses_ephemeral_handle`; plan WP-A contract | pass | Runs may continue after disconnect only until the five-minute timeout; ephemeral status/output expires after ten minutes. Daemon restart invalidates handles and never replays a command. |
| Sandbox and human confirmation policy remain daemon-owned | Persisted sandbox-profile lookup and `evaluate_command` in `src/core/daemon_human_shell.rs`; canonical managed-process launch specification | pass | Constrained profiles fail admission if the launcher cannot enforce them; explicit `FullHost` remains an operator-selected no-containment profile. |
| C001 promotion and model-context boundaries remain intact | Existing C004 closure `plans/closure/human-execution-corrective/004-status.md`; origin DTO and audit digest do not project raw output | pass | Bang output remains private by default; promotion remains under the C004 durable admission contract. |
| Existing PTY ownership and behavior remain separate | `src/tui/commands/shell.rs`, `src/interactive_process.rs`; hosted full workspace suite | pass | C002 does not change PTY DTOs or move PTY process ownership. |

## 3. Production implementation evidence

The additive `human_shell.v1` protocol provides capability discovery, bounded create/read/cancel operations, scoped identifiers, output cursors, and explicit errors. The daemon resolves authenticated principal, registered workspace, persisted project/session binding, host shell, canonical cwd, and persisted sandbox profile. It rechecks scope for reads and cancellation and audits a digest of the command rather than command text or output.

The in-memory run service uses the existing `ManagedProcessService` and the shared scheduler admission controller. It limits concurrency, command duration, retained output, chunk count, and handle lifetime; cancellation kills the managed process. Submission retries are idempotent only when request content and scope match. No storage migration or second durable command-history system was added.

The TUI now adapts bang commands to daemon requests and renders typed results. Dangerous-command confirmation remains a human UI action and is reevaluated by the daemon. `--standalone` uses the same core-owned handler. There is no TUI-local process fallback.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --lib shell::daemon --locked
cargo test -p codegg --lib poolless_standalone_human_shell_uses_registered_workspace --locked
python3 scripts/check_authorization_matrix.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_audit_invariants.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
cargo check -p codegg --features server --locked
```

### Results

- Workspace Clippy passed locally with `-D warnings` after the two hosted Clippy findings were corrected.
- Focused daemon service tests: 5 passed; standalone registered-workspace daemon-handler test: 1 passed.
- Authorization, audit coverage/invariants, execution ownership, scheduler bypass, formatting, and `verify.sh quick` passed during implementation. The quick verification was run before the final Clippy-only cleanup; exact-SHA hosted CI then reran all configured guards, formatting, Clippy, and workspace tests on the final implementation SHA.
- `cargo check -p codegg --features server --locked` passed. An unrelated existing deprecation warning in `src/server/ws.rs` was observed.
- A local `cargo test --test interactive_terminal_tui --locked` attempt was interrupted during shared build-directory contention; it was not a test failure. The exact-SHA hosted workspace suite includes the integration target and passed.
- Hosted CI run `38263761126` passed all configured guard and lint steps and the full sweep: 12,789 passed, 5 skipped, 0 failed; test sweep duration 394.816 seconds.
- Hosted Desktop E2E run `38263761188` passed the production desktop checks and built-app WebDriver trajectory.

## 5. Invariant review

- The frontend supplies command intent and explicit workspace/session identity only; daemon authority determines principal, workspace root, shell, environment, sandbox profile, and node locality.
- Human bang execution remains separate from model `ToolBroker`, interactive PTY handles, scheduler job identities, and session identities.
- `ManagedProcessService` remains the finite process lifecycle owner; shared scheduler admission is required before accepting work.
- Raw process output remains bounded and ephemeral and is not written into session observation or generic audit events.
- No cross-node execution is inferred. Unsupported remote-node targets fail explicitly.

## 6. Failure and recovery review

Duplicate creation with the same request identity is idempotent; changed content or scope returns conflict. Unknown, expired, or foreign run handles are unavailable for reads/cancellation. Cancellation uses the managed process handle. Daemon restart drops ephemeral run handles and never retries commands. Output truncation advances a cursor and is represented explicitly rather than backfilled as complete. Admission contention refuses before spawn.

## 7. Migration and compatibility review

No schema migration was needed. Protocol additions are capability-gated. Unsupported peers refuse the operation, and there is no direct local fallback. Existing PTY APIs and snapshot/history behavior are unchanged.

## 8. Security review

Authorization is evaluated against `command.execute` and authenticated transport authority. Workspace and session bindings are checked at admission and rechecked for read/cancel. Command warning/block classification is followed by human confirmation where required and daemon reevaluation. Command text is hashed for audit; raw output and credentials are not logged. Execution limits bound process duration, concurrency, output, cursors, and retention. A sandbox profile that cannot be enforced fails admission.

## 9. Documentation and operations

Updated `architecture/human_shell.md`, `architecture/process-tool-execution-ownership.md`, `architecture/authorization.md`, `architecture/protocol.md`, `architecture/tui.md`, `docs/tui.md`, `docs/execution-ownership.toml`, and the human-shell and TUI skills. Static ownership and scheduler guards passed. Remote-node execution remains explicitly unsupported.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Remote-node human-shell federation is not implemented. | Commands for a workspace bound to a remote execution node are refused. | Separate architecture and implementation plan if federation is authorized; no action is required for C002 closure. |

## 11. Roadmap disposition

C002 is closed. The workspace-owned finite human-shell capability is available for the supported daemon-local and standalone topologies. Its remote-node limitation is an explicit scope boundary, not an unresolved correctness condition.

## 12. Registry updates

The implementation plan, human-execution roadmap, both C002 registry rows, and the active subsystem summary are updated to closed. A dependency audit found no registered downstream plan newly unblocked by C002. The registry’s fresh-planning table remains empty; Windows native sandbox C011/M004 remains blocked on privileged adversarial evidence.
