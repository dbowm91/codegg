# Human Execution Corrective C002 — Workspace-Owned Human Shell Dispatch

Status: blocked (handoff after C001 accepted context/provenance contract)

Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b`

Source roadmap: `plans/subsystems/human-execution-corrective-roadmap.md#c002--workspace-owned-finite-shell-dispatch`.

Hard predecessor: `plans/implementation/human-execution-corrective/001-human-shell-promotion-and-redaction.md`. Existing ownership foundation: `architecture/process-tool-execution-ownership.md`, `plans/closure/interactive-process-sessions/002-status.md` and `plans/closure/tui-project-sessions/005-status.md`.

Long-term requirements: `plans/000-long-term-specification.md#45-locality-by-default`; `#17-job-scheduling-and-execution-backends`; `#18-remote-projects-and-execution-targets`; `#29-system-invariants`. Applicable ADRs: none for local daemon ownership; remote cross-node execution architecture is a separate decision.

Primary class: infrastructure/capability.

## 1. Objective

Run finite human `!` commands on the authoritative execution node of the selected workspace, never accidentally on the machine hosting a remote TUI, while retaining no-agent semantics, explicit human authorization, finite process cancellation and bounded output. Integrate C001's trusted session/context-promotion contract unchanged.

## 2. Why this milestone is blocked but implementable after C001

Process management, daemon `CoreClient`, workspace execution context, scheduler admission and bounded interactive process wire patterns are available. However, moving raw command execution must not create a competing model-context store/projection identity before C001 settles that ownership. C001 is a strict hard dependency; once closed, no outside code is required for daemon-local execution. New remote-execution fabric is not part of the milestone.

## 3. Current implementation evidence and earlier test gap

`src/tui/app/prompt_turn.rs` resolves a composer workspace root then emits `TuiCommand::RunHumanShell`. `src/tui/commands/shell.rs::spawn_human_shell` instantiates `ShellRuntime::new()` in the frontend, and `src/shell/runtime.rs` uses `ManagedProcessService::run_streaming` there for `$SHELL -lc`. On remote frontend/daemon topologies, the selected path can refer to a different machine and the inherited `SHELL`/environment comes from the client. By contrast, `/terminal-*` calls `CoreRequest::InteractiveProcess*` on the daemon. Prior human shell tests exercised a local process and checked cwd/output, not an independent daemon/client identity.

## 4. Invariants

- Authenticated workspace/node ownership determines execution; client current directory, `SHELL`, environment and arbitrary client path cannot bestow command authority.
- Human finite commands remain distinct from model `ToolBroker`/`bash`, PTY interactive handles, durable scheduler `Job` identities and human `Session` IDs.
- `ManagedProcessService` remains the one finite spawn/lifecycle owner; scheduler policy applies before acceptance where required; no parallel custom process engine or unbounded background process.
- Only explicitly allowed inherited variables reach spawned shell; strip unsafe Git/loader variables and credentials; never trust frontend env.
- `!` stays model-private; `!!` follows C001's redacted, bounded, session-bound single projection path; raw streams never become session observation by default.
- Unknown/unsupported remote targets fail visibly, not via silent local-process fallback.

## 5. Scope

In: typed daemon shell-run/stream/cancel request family, provenance and authorization, workspace resolution, TUI as adapter, compatibility/fallback errors, cancellation/reconnect, tests/docs. Out: remote node transport/federation, SSH tunneling, long-lived persistent `!` shell, PTY engine changes, agent tools, new process supervisor/scheduler, Windows PTY.

## 6. Required production changes

### Core/domain
Typed human finite command request with workspace ID, bounded command, explicit origin/owner, command/run ID, execution node identity and cancellation metadata. Server constructs authoritative principal and `ExecutionContext` from authenticated transport and registered workspace; request strings cannot claim human authority. Treat command as executable script for `$SHELL -lc` preserving user semantics, not untrusted argv from the model.

### Storage and migrations
Prefer ephemeral run state and existing audit machinery; no second command-history database. Raw bytes remain bounded/ephemeral, promoted results use C001's canonical context owner. Reconnect must return bounded retained status/stream or explicit unavailable/expired state, not fabricated history.

### Protocol and DTOs
Use a versioned/capability-gated family: create finite human run, bounded output read/subscribe with sequence/cursor and redacted metadata, cancel/status. Derive stable IDs from trusted admission; enforce length, byte, timeout, concurrency and workspace limits; attachment/read/cancel authorization tied to human principal, session and workspace. Audit existing `CoreRequest` and streaming DTOs first to avoid a duplicate protocol. Older clients/daemons must decline unsupported locality rather than execute on the wrong host.

### Runtime and concurrency
Daemon/execution owner resolves workspace to immutable context and host-owned shell/environment, consults the existing scheduler admission policy, and invokes `ManagedProcessService::run_streaming`. Keep process tree termination/reaping, timeout and output-limit behavior. Choose an explicit client disconnect contract (bounded continue-with-expiry or cancellation-on-owner-disconnect) and test it, with no orphan run. Ensure cancellation of a completed/unknown run is idempotent or typed gone. Captured async completions are generation/route checked.

### Frontend/operator
TUI submits intent through core, projects typed events to existing `ShellCell` and `ShellOutputStore` compatibility surfaces, and reports workspace/execution node provenance. Preserve `!`/`!!`, list/show/kill/rerun/include and C001 staged promotion. Tab switches never redirect a running command; a re-run must obtain fresh authority under the selected workspace. Local `--standalone` follows the core-owned adapter rather than keeping a separate direct TUI spawn path.

### Security and authorization
Existing `evaluate_command` Warn/Block classification is a human UX safety feature, not a sandbox. Enforce real execution privilege, sandbox/authorization policy and identity at daemon; observer/foreign principals cannot submit, read raw bytes or cancel. Environment inheritance must be from execution owner and filtered; never log credentials or command output to generic projection/audit. Permissions for remote human execution must not be inferred from session-control read capability.

### Documentation/static guards
Reconcile `architecture/human_shell.md`, `architecture/process-tool-execution-ownership.md`, `architecture/protocol.md`, `architecture/tui.md`, `docs/tui.md`, human-shell and TUI skills, and `docs/execution-ownership.toml`. Keep new spawn sites aligned with `scripts/check_execution_ownership.py` and `scripts/check_scheduler_bypass.py`. No new CI lane.

## 7. Ordered work packages

A. Freeze transport/client/daemon execution topology and effective identity, audit admission and current client/daemon command flows. Record a support matrix for local daemon, standalone core, remote-attached TUI and remote workspace target.
B. Add bounded typed daemon command execution/stream/cancel contract using canonical workspace and principal resolution and the existing managed-process service.
C. Convert TUI bang handler into a thin client of the daemon family. Preserve display/history compatibility and C001 redaction/promotion; make unsupported locality explicit.
D. Prove two-client/two-workspace execution, cancellation and reconnect behavior, repair docs/skills and record closure evidence.

## 8. Failure, cancellation, restart and contention

Admission refusal must occur before spawn; nothing claims “running” on refusal. Distinguish timed out, cancelled, output-limit and lost process. Disconnect policy must limit lifetime; daemon restart invalidates ephemeral handles. Out-of-order output, duplicate run creation, stale session generations and foreign cancellation cannot create another process or cross-project output leak. Saturation yields bounded lag/resync, typed truncation or refusal. Don't backfill missing output as if complete.

## 9. Compatibility and migration

Shell syntax and human output-history semantics remain. New protocol features are additive and negotiated; missing support yields refusal. No model tool name/permission history changes. Existing interactive-process DTOs and PTY owner remain unchanged. No persistent schema migration unless demonstrated necessary.

## 10. Required tests

- Two-client fixture with frontend cwd/environment intentionally differing from daemon workspace; `pwd`/sentinel prove only authoritative execution-root identity is used. Two workspaces isolate cwd, env, command handles and output under concurrency.
- Negative: forged workspace/principal, observer, unbound/archived workspace, unknown execution node, oversized command, invalid env, absent protocol capability and remote-only target fail closed.
- Timeout, output caps, streaming order/lag, process tree cleanup, cancel/late exit, duplicate start ID, connection loss and daemon restart.
- C001 regression: `!` never enters model; `!!`/include project only authenticated bounded redacted output in the intended session and survive the relocation.
- Existing `tests/interactive_terminal_tui.rs` remains passing, proving no effect on PTY owner.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib shell
cargo test --test interactive_terminal_tui
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
```

Add focused daemon/client authorization and stream lifecycle tests; report exact executed tests in closure. Do not require a real remote host for deterministic local qualifications or claim remote federation.

## 12. Documentation updates

Document execution-node, shell environment source, protocol capability, standalone/remote limitations, and non-agent ownership in architecture, skills, CLI/TUI docs and execution ownership manifest.

## 13. Acceptance criteria

Human one-shot commands execute only under the selected workspace's authorized execution owner, never the attaching client's host by accident; foreign workspaces/observers fail; cancellation/reconnect bounded; historical PTY/agent semantics and C001 context isolation remain intact.

## 14. Stop conditions

C001 not closed; process/scheduler ownership unresolved; platform requires inventing unsupported remote execution; no adequate authorization proof; requirement to copy unredacted model output. Split new architecture rather than silently fall back.

## 15. Closure evidence

Protocol/auth matrix, two independent client contexts and two-workspace cwd/environment marker capture, cancellation/restart/contended-run tests, ownership manifest/guard results, skill/docs updates, exact test outputs and residual limitations.

## 16. Handoff notes

Do not conflate daemon-local workspace ownership with future remote execution across nodes. Recheck source baseline at start; preserve user changes and historical M001–M003 PTY closure.
