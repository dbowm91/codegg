# Human Execution Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/human-execution-corrective/002-workspace-owned-human-shell-dispatch.md`

Source subsystem roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c002--workspace-owned-finite-shell-dispatch`

Repository baseline reviewed: `bdaa9f822bd077e070947a9ea55f8a48a1d4eb2c`

Implementation commits:

- `f9d999deddf9cc07c00f3a40c89f55651552736a` — daemon-owned workspace-bound finite human shell protocol, TUI adapter, authorization/audit/ownership records, and documentation.

## 1. Executive finding

C002 is complete. Human `!` commands now run through the connected CoreDaemon
that owns the selected registered workspace. The frontend sends command intent
and workspace/session locators; it cannot select the shell, cwd, environment,
principal or execution node. Unknown/archived workspaces and unbound sessions
are refused. The TUI has no frontend-local execution fallback.

The qualified topologies are local daemon, standalone in-process core, and a
remote-attached TUI whose connected daemon has the selected workspace. A
workspace assigned to another node remains unsupported and is refused; this
milestone adds no cross-node transport or live remote-host claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Execute at the registered workspace owner | `human_shell_dispatch_uses_registered_daemon_workspace`; `two_workspaces_execute_on_owner_and_raw_output_is_owner_scoped` | pass | `pwd` and distinct markers came from two daemon workspace roots. |
| Enforce trusted human/session/workspace authority | daemon request handler and `ViaSession + agent.invoke` descriptor; `non_human_transport_cannot_negotiate_or_create_human_shell`; daemon dispatch test rejects unbound, unknown and archived workspace requests | pass | Request DTO contains no principal or node claim. Foreign owner/session reads return the same typed `human_shell_run_gone`. |
| Bound runtime, output and concurrency | `daemon_run_timeout_is_reported_as_timed_out`; `retained_output_is_bounded_and_marks_truncation`; `concurrent_creates_respect_per_principal_active_limit`; environment allowlist unit test | pass | 300-second maximum, 1 MiB retained output, 64 KiB reads, four active runs per principal, and filtered daemon-owner environment. |
| Preserve cancellation, retries and restart behavior | `cancellation_and_request_bounds_are_enforced`; `create_retry_is_idempotent_and_conflicting_reuse_is_refused`; `dropping_daemon_protocol_kills_live_run_and_releases_permit` | pass | Cursors are contiguous; cancellation during startup and daemon shutdown are bounded; handles are process-local. |
| Preserve private output and redacted promotion | existing shell promotion regression suite under `cargo test -p codegg --lib shell`; C001/C004 closure records | pass | Raw run output does not enter session or generic audit storage; promotion remains the existing owner-redacted path. |
| Preserve PTY ownership and presentation | `cargo test --test interactive_terminal_tui` | pass | Six PTY lifecycle, workspace-routing, resync and restart tests passed. |
| Keep execution ownership, scheduler and audit boundaries | `check_execution_ownership.py`, `check_scheduler_bypass.py`, `check_audit_coverage.py`, and `verify.sh quick` | pass | New process site is declared; admission is shared; successful create emits a command digest audit event. |

## 3. Production implementation evidence

`HumanShellRun` adds a versioned capability/create/read/cancel protocol. The
daemon resolves the transport-bound principal, session runtime and workspace
registry, rejects archived or unknown workspaces, creates an immutable
`ExecutionContext`, and acquires the shared scheduler process permit before
calling `ShellRuntime` and `ManagedProcessService`.

The daemon keeps only bounded process-local run records and raw output. Records
are scoped to the creating principal and session, use length-delimited
idempotency keys, expire after ten minutes, and are capped per principal and
daemon. The TUI negotiates the capability, creates the run, reads bounded
sequence cursors, projects truncation into the shell cell, and sends cancel
through the same owner protocol. The displayed cwd is the daemon workspace ID,
not an attaching client's path.

Successful create emits the standard `command_execute` event with a structural
command digest and run locator. Raw command text and output are excluded from
generic audit/session storage. The new site is recorded in
`docs/execution-ownership.toml` and the daemon execution hook is pinned.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib shell
cargo test -p codegg --lib shell::human_dispatch
cargo test -p codegg --lib human_shell
cargo test --test interactive_terminal_tui
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_audit_coverage.py
scripts/verify.sh quick
```

### Results

- `cargo test -p codegg --lib shell`: 520 passed, 0 failed.
- `cargo test -p codegg --lib shell::human_dispatch`: 7 passed, 0 failed,
  including timeout, truncation, cursor order, concurrency, cancellation,
  idempotency and shutdown cleanup.
- `cargo test -p codegg --lib human_shell`: 7 passed, 0 failed, including
  daemon workspace, archived/unknown workspace, unbound session and
  non-human transport checks.
- `cargo test --test interactive_terminal_tui`: 6 passed, 0 failed.
- The three focused guards passed. `scripts/verify.sh quick` passed, including
  formatting, all repository boundary and routing guards, and
  `cargo check --workspace --all-targets --locked`.
- No native remote host or cross-node execution was used or claimed. The
  deterministic daemon-owned tests qualify the local/attached protocol path.

## 5. Invariant review

- Human shell remains distinct from model tools, durable jobs, PTY handles and
  session identities.
- Workspace authority comes from the daemon registry; client cwd, shell and
  environment are not execution inputs.
- Managed process lifecycle remains in `ManagedProcessService`; scheduler
  admission is held until process completion and reaping.
- `!` output remains private. Explicit promotions continue through C001/C004's
  redacted, bounded, session-bound owner path.
- Unsupported execution targets fail explicitly without frontend fallback.

## 6. Failure and recovery review

Idempotent create retries return the original run; conflicting reuse is
refused. Cancellation is safe during startup and takes precedence over a late
terminal event. Output reads are bounded and carry sequence cursors; truncation
is visible to the TUI. A disconnected frontend may continue polling while the
daemon keeps the bounded run alive through its configured timeout. Daemon
shutdown kills live handles and releases permits. Run/output history is
ephemeral and is not reconstructed after restart.

## 7. Migration and compatibility review

No storage migration was required. Four CoreRequest and four CoreResponse
variants were added; capability version 1 gates the feature. Older peers or
unsupported daemons return an error and never trigger local execution.
Existing `!`/`!!` and promotion syntax remains. No change was made to PTY or
agent tool identities.

## 8. Security review

Human/LocalOwner principal checks are applied after transport-bound
authorization; authenticated session operations require `agent.invoke`.
Session/workspace mismatch, unbound session, unknown workspace, archived
workspace and foreign run access fail closed. Request lengths, timeout, cursor
read size, active runs, retained records and output are bounded. The inherited
environment is drawn from the daemon owner and filtered to an explicit
allowlist; credential, loader, and `GIT_*` variables are removed. No secrets,
command text or output are added to audit metadata. Human command warnings
remain a UI safeguard, not an OS sandbox; this is explicit human execution
under the daemon owner's existing privilege.

## 9. Documentation and operations

Updated `architecture/human_shell.md`, `architecture/process-tool-execution-ownership.md`,
`architecture/protocol.md`, `architecture/tui.md`, `architecture/audit.md`,
`docs/tui.md`, `.opencode/skills/human-shell/SKILL.md`, and
`docs/execution-ownership.toml`. No new guard or CI lane was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None within C002's accepted boundary. | — | — |

Cross-node execution remains an explicit non-goal. The separate sandbox
corrective C004 remains blocked on native Darwin qualification as recorded in
`plans/registry.md`; C002 does not satisfy or weaken that evidence gate.

## 11. Roadmap disposition

C002 is closed. All milestones in the human-execution corrective roadmap are
closed; the subsystem roadmap may be marked closed. No C002-dependent plan is
blocked or newly unblocked.

## 12. Registry updates

- Mark C002 closed in its plan and roadmap; mark the human-execution corrective
  roadmap closed.
- Remove C002 from dependency-ready plans and record it under recently closed
  work with implementation commit `f9d999de`.
- Audit found no other registered plan depending on C002. Sandbox C004 remains
  blocked on native Darwin evidence; no status changes there.
