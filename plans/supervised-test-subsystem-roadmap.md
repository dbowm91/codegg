# Supervised Test Subsystem Roadmap

## Purpose

This roadmap defines the minimal path for adding deterministic, supervised test execution to codegg. The immediate goal is to reduce LLM overhead during long-running or noisy test runs by moving process supervision, output capture, timeout classification, and first-pass failure summarization into codegg itself.

The minimal version should not try to build the final fully detached background-job architecture. It should introduce a model-facing `test` tool that runs tests through a streaming supervised runner, preserves complete logs on disk, and returns a compact actionable report to the agent. TUI progress events and slash-command ergonomics can be layered on once the runner and tool are stable.

## Current architecture observations

codegg already has the correct high-level seams for this work.

The model-facing tool surface is centralized in `src/tool/`. The `Tool` trait supports regular string execution and optional `execute_structured`, while `ToolRegistry::with_options` is the authoritative registration path for built-ins. A `test` tool should be implemented as a first-class native tool rather than as prompt guidance around `bash`.

The existing shell execution tools are not the right substrate for progress-aware testing. `BashTool` and `TerminalTool` execute through `tokio::process::Command`, wrap execution in `tokio::time::timeout`, and await `cmd.output()`. That collects stdout and stderr only after process completion. It is fine for short shell commands, but it prevents deterministic progress tracking, no-output stall detection, incremental failure extraction, and TUI live updates.

The agent loop already recognizes test-like bash commands through an `is_test_command` helper covering `cargo test`, `cargo nextest`, `pytest`, `uv run pytest`, `go test`, `make test`, and similar commands. That should be treated as evidence that tests deserve semantic treatment rather than raw shell treatment.

The event bus already carries agent/tool lifecycle events. A minimal supervised runner can initially return synchronously through the `test` tool, then later publish `TestRunStarted`, `TestRunProgress`, and `TestRunCompleted` events for TUI and daemon clients.

The command system already supports built-in slash commands and process-backed commands. `/test` should be added after the core runner exists, not before.

## Target minimal architecture

The first useful version should introduce this shape:

```text
src/test_runner/
  mod.rs
  types.rs
  resolve.rs
  runner.rs
  parse.rs
  report.rs

src/tool/test.rs
  model-facing `test` tool

src/tool/mod.rs
  register TestTool

optional follow-up:
crates/codegg-core/src/bus/events.rs
  TestRunStarted / TestRunProgress / TestRunCompleted
```

The initial control flow should be:

```text
LLM calls test tool
  -> TestTool validates JSON input
  -> TestResolver maps scope to command
  -> TestRunner spawns process with piped stdout/stderr
  -> Runner streams output into full log files
  -> Parser extracts progress and failures opportunistically
  -> Runner enforces wall-clock and no-output timeout
  -> Reporter returns compact TestReport text to LLM
```

The LLM should not receive the full test stream by default. It should receive a bounded report with status, command, duration, exit code, failure class, top failures, timeout classification, and log path.

## Minimal supported scopes

The first version should support these tool scopes:

```text
auto
workspace
changed
package
file
previous_failures
custom
```

For the first implementation, `changed` can fall back to package/workspace testing if git-aware impact resolution is not ready. `previous_failures` can initially read the last report index and rerun a stored command. `custom` should still be treated as test execution and supervised by the same runner.

## Minimal language support

The first version should support:

```text
Rust:   cargo test, cargo test -p <package>, cargo nextest if explicitly requested later
Python: pytest, uv run pytest if detected or explicitly requested
Generic: custom command with supervised process behavior
```

Detection should remain conservative. If `Cargo.toml` exists, default to Rust. If `pyproject.toml`, `pytest.ini`, `tox.ini`, `noxfile.py`, or a `tests/` directory exists, default to pytest. If multiple ecosystems exist, prefer explicit scope or return a deterministic ambiguity error listing candidate commands.

## Minimal report behavior

The model-facing report should be short and stable. It should contain:

```text
status
command
cwd
duration
exit code if available
failure class
primary failures
last meaningful output on timeout
full log path
whether output was truncated
```

The report should avoid dumping full stdout/stderr. Full logs should be stored under `.codegg/test-runs/<timestamp>-<job-id>/` with at least `stdout.log`, `stderr.log`, and `report.json`.

## Timeout and stall policy

The minimal implementation should distinguish:

```text
wall_clock_timeout
no_output_timeout
process_spawn_error
nonzero_exit
cancelled
```

No-progress timeout can be deferred until language parsers are reliable enough to track test counters. Wall-clock and no-output timeout provide most of the immediate value.

The runner must kill the process on timeout. On Unix, this should eventually be process-group aware; the first pass may use `kill_on_drop(true)` plus explicit kill, but the plan files require a hardening follow-up for child-process cleanup.

## Security and permission posture

The `test` tool executes project code. It must not be categorized as read-only. For the first version, classify it as `ToolCategory::ShellExec` unless a better dedicated `TestExec` category is added. This keeps it near the current shell permission model and avoids presenting test execution as a harmless diagnostic.

The `custom` scope should receive stricter handling than resolver-generated commands. It should reuse the same dangerous-pattern checks or route through a shared shell-policy helper if one is extracted. Do not copy-paste a divergent blocked-pattern implementation into the test tool.

## Relationship to bash and terminal

Do not retrofit `bash` or `terminal` first. The existing shell tools have stable behavior and a broader threat model. The test subsystem should use a new streaming runner and may later share lower-level process supervision utilities with shell tools.

After the `test` tool lands, update tool descriptions and agent guidance so the model prefers `test` for `cargo test`, `pytest`, and equivalent test commands. A later corrective pass can detect model calls to `bash` with test-like commands and suggest or internally route to `test`, but this should not be the first milestone.

## Relationship to RTK and context artifacts

RTK/context integration should not block the minimal runner. The first version should return filesystem log paths. The second version can optionally store large logs as context artifacts and return expandable `ctx://` handles through the existing context artifact system.

The eventual RTK-backed state should store:

```text
last successful test command
last failure signatures
known slow tests
known flaky tests
file-to-test/package mapping
recent timeout classifications
```

The first five plan files intentionally stop before this persistent intelligence layer.

## Milestones

### Milestone 1: Internal types, resolver, and parser skeleton

Create the test runner module, define stable data types, implement conservative Rust/Python command resolution, and add parser skeletons with unit tests. No process spawning is required yet.

### Milestone 2: Streaming supervised process runner

Implement the async runner that pipes stdout/stderr, writes full logs, enforces wall-clock/no-output timeouts, records process metadata, and produces a raw report object.

### Milestone 3: Model-facing `test` tool

Expose the runner through `src/tool/test.rs`, register the tool, add schema/description/category, and return compact report text. Add integration tests for tool invocation.

### Milestone 4: Failure extraction and report quality

Improve Rust and pytest parsing enough that common assertion failures, panics, compile errors, pytest failures, and timeout last-output contexts produce useful compact summaries.

### Milestone 5: TUI/event-bus visibility and `/test` command

Add test lifecycle events and minimal TUI command plumbing so users can run `/test` directly and observe progress without involving the LLM.

## Acceptance criteria for the minimal subsystem

The initial supervised test subsystem is acceptable when:

```text
- The model can call `test` instead of `bash` for common Rust/Python test runs.
- The runner streams stdout/stderr to disk rather than collecting output only at process completion.
- Full logs are retained outside model context.
- The model receives a compact bounded report.
- Wall-clock timeout is classified distinctly from nonzero test failure.
- No-output timeout is classified distinctly from wall-clock timeout.
- Rust and pytest common failures are summarized without full log dumping.
- The implementation has unit tests for resolver/parser behavior and integration tests for timeout/report behavior.
```

## Deferred items

These should not block the minimal version:

```text
- Fully detached background jobs that resume the agent after completion.
- Process-group hardening across all platforms.
- RTK-backed test memory and impact analysis.
- Coverage-aware or LSP-aware test selection.
- nextest JSON/JUnit integration.
- pytest plugin integration.
- automatic rerouting from `bash` test commands to `test`.
- daemon persistence of active test jobs across restarts.
```

## Risks

The main risk is over-scoping. The first version should be a supervised synchronous tool with streaming capture and compact reporting. A fully evented background job system is desirable, but it touches agent turn resumption semantics, daemon state, TUI task state, cancellation, and persistence. Those are separate follow-up milestones.

The second risk is misclassifying `test` as safe/read-only. Tests execute arbitrary repository code and may mutate local files, open network sockets, or consume resources. Keep the permission posture conservative.

The third risk is duplicating shell-security logic. The implementation should either reuse existing shell policy helpers or extract common checks before allowing arbitrary `custom` commands.
