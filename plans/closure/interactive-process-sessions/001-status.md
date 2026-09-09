# Interactive Process Sessions Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/interactive-process-sessions/001-scheduler-owned-pty-engine.md`

Source subsystem roadmap:

- `plans/subsystems/interactive-process-sessions-roadmap.md#M001--scheduler-owned-PTY-engine`

Repository baseline reviewed: `e7b017bde5775547d809cad84dc8f34304f1e87`

Implementation commits or pull requests:

- `95a56d6c` — feat(interactive): M001 scheduler-owned PTY engine with bounded scrollback and group cleanup

## 1. Executive finding

M001 is complete as infrastructure: CodeGG has a local scheduler-owned
PTY engine (`src/interactive_process.rs`) that spawns genuinely
interactive workspace processes under an immutable execution context and
a scheduler permit acquired before `openpty`, accepts bounded input and
resize, streams bounded sequence-numbered output, terminates the whole
process group with SIGTERM-then-SIGKILL escalation, releases scheduler
capacity on exit/terminate/shutdown, and retains no durable state across
daemon restart. No public/model tool is registered; the deferred
one-shot `terminal` tool is untouched. M002 may proceed.

Baseline note: the plan cites `9dfbc6e1`; the implementation was built on
`e7b017bd` (later `main`, identity M005 closure). No drift affects M001
dependencies — scheduler, workspace/execution-context, and managed
non-interactive process foundations are unchanged in the respects M001
consumes.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| PTY backend over smallest platform layer (§7B) | `open_pty_pair` via `libc::openpty` (already a direct dependency); no new crate, no multiplexer | pass | `src/interactive_process.rs` |
| State machine / handle / resource limits / platform contract (§7A) | `SessionState` (Starting→Running→Terminating→Exited), UUID handles, scrollback/input/size/env bounds, `is_supported`/`platform_name` | pass | Unit tests + snapshots |
| Scheduler admission, no bypass (§6, §8) | `try_admit_arc` before any PTY/child creation; permit guard held in session, dropped on exit/terminate/shutdown | pass | Contention + release tests |
| Workspace/environment policy (§6) | `Arc<ExecutionContext>` required; cwd via `resolve_relative_cwd`; sanitized policy reuse + PTY loader-injection overlay | pass | Escape/absolute-cwd/env tests |
| Input / resize / exit status (§6) | `write_input` (32 KiB/write), `resize` (ioctl + SIGWINCH), `ExitInfo` from reaped child | pass | Round-trip/resize/exit-7 fixtures |
| Output sequence / ring buffer (§6) | `SequenceRing` with global `next_seq`, gap-reporting `read_from` | pass | Unit + truncation fixtures |
| Terminate/kill escalation (§8) | SIGTERM to process group, SIGKILL after `TERMINATE_GRACE` | pass | Trap-escalation fixture |
| Daemon shutdown cleanup (§8) | `shutdown()` terminates all sessions bounded, rejects new spawns | pass | Shutdown fixture |
| Spawn failure releases permit, no live handle (§8) | `drop(permit)` on child-creation error path | pass | Missing-binary test asserts slots return to 0 |
| Crash/ephemeral semantics (§8) | No durable record; restart reports prior handles gone (handles live only in memory) | pass | By construction; `remove()` drops scrollback |
| Contention queues/refuses, never spawns anyway (§8) | `AdmissionBlocked` without spawn; session count unchanged | pass | 1-slot contention test |
| Additive only; `terminal` tool unchanged (§9) | No tool registration; `terminal.rs` untouched | pass | `git diff` scope |
| Docs: process ownership map (§6) | Manifest entry + architecture rows + Session-vs-handle section | pass | See §9 |
| Real interactive fixture (§10, §15) | `cat` round-trip, `sh` command + exit code, `stty size` observation | pass | `tests/interactive_process_sessions.rs`, 11/11 |
| Large output bounds (§10) | 300 KiB through default 256 KiB ring; truncated, cursors stable | pass | Fixture asserts retained ≤ cap |
| Process-group child cleanup (§10) | Background `sleep 60` descendant reaped via group signal | pass | ESRCH-poll fixture |
| Supported platform fixture (§10) | `platform_fixture_reports_supported_unix_host` on macOS arm64 | pass | Linux path shares code; see §10 low finding |

## 3. Production implementation evidence

- **New owner**: `src/interactive_process.rs` (~1500 lines incl. tests).
  `InteractiveProcessService` (daemon/execution-node owned) maps ephemeral
  UUID handles to `LiveSession`s: `AsyncFd` PTY master, tokio child with
  slave stdio + `setsid`/controlling-terminal `pre_exec`, background
  reader (sequence ring) + waiter (exit capture, permit release) tasks.
- **Admission**: `permit_dimensions_for_interactive()` documents the
  contract — ManagedProcess resource class, one process slot, no
  exclusivity key. Guard stored per session; released exactly once via
  `finish_session`.
- **Workspace binding**: spawn takes `&Arc<ExecutionContext>`; no
  `current_dir` anywhere in the module (cwd guard passes). Absolute cwd
  rejected; relative cwd resolved under the workspace root.
- **Environment**: canonical `EnvironmentPolicy::sanitized()` reused via a
  one-line `pub(crate)` visibility widening in `src/managed_process.rs`
  (no behavior change); interactive `TERM=xterm-256color` override;
  `CODEGG_INTERACTIVE_PROCESS/HANDLE/WORKSPACE_ID` provenance; extra
  `PTY_HARD_DENIED_ENV_VARS` overlay drops `LD_PRELOAD`,
  `LD_LIBRARY_PATH`, `DYLD_*` overrides (the Git-scoped canonical deny
  list does not cover loader injection; rationale documented at the
  constant).
- **Non-goals honored**: no `JobKind`/`JobRecord` (ephemeral by design),
  no protocol DTOs (M002), no TUI (M003), no sandbox changes, no
  `shell_session` authority change, no second supervisor (finite argv
  work stays with `ManagedProcessService`).
- **Dependency rationale** (§15): zero new dependencies. `libc::openpty`
  exists on Linux and macOS (verified signatures differ only in
  constness; handled with `target_vendor` cfg). `tokio` AsyncFd +
  `tokio::process::Command::pre_exec` cover async master I/O and safe
  fork/exec without a PTY crate. A multiplexer/session framework was
  deliberately not imported per handoff notes.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib interactive_process --no-fail-fast
cargo test --test interactive_process_sessions --no-fail-fast
cargo test -p codegg --lib managed_process --no-fail-fast
cargo test -p codegg --lib scheduler --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
bash scripts/verify.sh quick
python3 scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
```

Substitution note: the plan's `cargo test --workspace <name>` lines use
crate names that do not exist in this workspace (`interactive_process`,
`managed_process`, `scheduler` are modules of the root crate, not
workspace members). The commands above are the exact narrowest
equivalents: module-filtered root-crate suites plus the dedicated
integration target. All are local-execution truth (no CI run claimed).

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib interactive_process` | pass — 13/13 |
| `cargo test --test interactive_process_sessions` | pass — 11/11 (real PTY fixtures, ~24 s) |
| `cargo test -p codegg --lib managed_process` | pass — 13/13 (no regression) |
| `cargo test -p codegg --lib scheduler` | pass — 77/77 (no regression) |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass (one initial `manual_range_contains` finding fixed, not suppressed) |
| `scripts/verify.sh quick` | pass (includes fmt, agent-asset check, core-boundary, sandbox contract, execution ownership, full `--all-targets` check) |
| core-boundary / execution-ownership / scheduler-bypass / daemon-cwd guards | pass |

## 5. Invariant review

| Plan §4 invariant | Evidence it remains true |
|---|---|
| Scheduler permit before spawn | Code order: validation → workspace policy → platform → `try_admit_arc` → `openpty`. Contention test proves no spawn without permit; failure test proves release. |
| Explicit WorkspaceId/root/execution target | `Arc<ExecutionContext>` is a required spawn argument; snapshot echoes `workspace_id` + `workspace_root`. |
| No process-global cwd authority | No `current_dir` in module; cwd guard passes; absolute cwd rejected. |
| Process group cleanup on terminate/shutdown | setsid session leader; negative-PID signals; descendant-reap + shutdown fixtures. |
| Bounded output and input | 256 KiB default ring (4 KiB–4 MiB configurable), 32 KiB/write input, 256 KiB/read, env count/size caps, size caps. |
| PTY lifecycle distinct from human Session | Handles are UUIDs; architecture note + module docs state the separation; no Session id is accepted or returned. |
| No model-facing tool registration | `ToolRegistry` untouched; no `Tool` impl in module. |
| No second general process supervisor | Only PTY slave-stdio spawn site; finite work still routes to `ManagedProcessService`; manifest classifies the file `interactive` with the permit rationale. |

## 6. Failure and recovery review

- **Spawn failure**: missing binary → `SpawnFailed`, permit dropped, `used_process_slots() == 0`, session count 0.
- **Reader/writer failure**: master EIO (slave closed) ends the reader cleanly as terminal output complete; write errors surface `WriteFailed`; input after exit is `NotRunning`.
- **Cancellation races**: `finish_session` takes the permit exactly once; concurrent `terminate` calls converge on `Terminating`→`Exited`; waiter + terminator coordinate via `Notify`, never unbounded waits (`TERMINATE_GRACE` + `SHUTDOWN_GRACE` caps).
- **Daemon restart**: handles are memory-only; a new daemon has an empty map, so prior handles report `UnknownHandle` (the typed "gone" answer until M002 defines wire resync).
- **Contention**: 1-slot controller refuses the second spawn with `AdmissionBlocked`; first session unaffected.
- **Malformed input**: empty argv, NUL bytes, oversized entries, zero/oversized dimensions, oversized env, absolute/escaping cwd all rejected before admission (order asserted by tests checking slot counts).
- **Bounded behavior**: ring truncation keeps newest bytes with gap flags; per-read caps; no unbounded task creation (two bounded tasks per session, both cancelled on finish).

## 7. Migration and compatibility review

- Additive only: one new module, one new integration test target, one `pub(crate)` visibility widening, one manifest entry, two architecture doc additions.
- No schema migration, no protocol change, no config change. Job/store/run formats untouched.
- `terminal` tool behavior unchanged; `shell_session` metadata untouched.
- Rollback: delete the module + test target + manifest entry; nothing else references the new code.

## 8. Security review

- **Authorization**: spawn requires a caller-held `ExecutionContext`; request payloads cannot claim workspace ownership. (Transport authority is M002 scope; the seam is the context.)
- **Secret handling**: environment *values* never logged (only handle/pid/size/shape at spawn); snapshots carry no argv beyond the executable name and no env at all.
- **Path validation**: cwd confined via `resolve_relative_cwd`; absolute paths rejected; `..` escapes rejected with slot counts asserted unchanged.
- **Privilege boundaries**: child is an ordinary user process in a fresh session; no setuid, no sandbox weakening (sandboxed finite execution is unchanged).
- **Denial-of-service bounds**: all queues/rings/reads/writes/resizes capped (§5); per-session task count fixed at two; admission bounds concurrency globally.
- **Loader injection**: `PTY_HARD_DENIED_ENV_VARS` overlay (new finding during implementation — the canonical Git deny list does not cover `LD_PRELOAD`/`DYLD_*`; close-out fix landed with the implementation commit and is covered by the env fixture asserting absence from `env` output).

## 9. Documentation and operations

- `docs/execution-ownership.toml`: new `interactive` site entry for `src/interactive_process.rs`.
- `architecture/process-tool-execution-ownership.md`: canonical-interactive disposition row + Session-vs-handle clarification.
- `architecture/scheduler.md`: ephemeral interactive admission contract subsection.
- Module rustdoc: ownership map, admission contract, platform matrix, bounds, state diagram.
- Operator diagnostics: `snapshot()` (state/size/cursors/exit/age, no secrets); `session_count()`; spawn logs handle/pid/size only.
- Platform matrix (§15): supported — Linux, macOS (`libc::openpty`); unsupported — non-Unix targets return `PlatformUnsupported` without spawning. Verified on macOS arm64 (this host); Linux shares the implementation behind a constness-only signature cfg (see §10 low finding).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Linux-host PTY fixture evidence not executed locally (this host is macOS). | Small: Linux `openpty` signature variant and PTY line discipline are unexercised until CI runs the new suites on Linux. | CI `verify` on Linux runs `interactive_process` + `interactive_process_sessions`; no code change expected. Not a closure blocker: the Unix implementation is shared. |
| — | No other findings. | — | — |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone M001 closed; M002 (bounded attach/resume protocol) may proceed —
its sole hard dependency (M001) is satisfied. M003 remains blocked on M002.
No subsystem roadmap revision required.

## 12. Registry updates

- `plans/registry.md`: remove the M001 row from dependency-ready plans;
  add M002 as dependency-ready (PTY engine closed); move the M002 row out
  of blocked work; advance the subsystem row to "M002 ready".
- `plans/subsystems/interactive-process-sessions-roadmap.md`: M001 →
  closed with closure link; M002 blocked → ready.
- `plans/implementation/interactive-process-sessions/001-scheduler-owned-pty-engine.md`:
  status → implemented.
- `plans/implementation/interactive-process-sessions/002-bounded-attach-resume-protocol.md`:
  status → ready for handoff.
