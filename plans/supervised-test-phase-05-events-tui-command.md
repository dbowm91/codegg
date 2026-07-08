# Supervised Test Subsystem Phase 5: Event Visibility and `/test` Command

## Objective

Add user-visible runtime plumbing for supervised test execution. This phase publishes test lifecycle events and adds a minimal `/test` slash command so users can run supervised tests directly from the TUI without requiring an LLM turn.

This phase should not implement fully detached agent-background test jobs. The model-facing `test` tool can remain synchronous. The new event/TUI path is for visibility, manual control, and later daemon/frontend integration.

## Precondition

Phases 1 through 4 should be landed:

```text
- Internal supervised runner exists.
- Runner streams logs and classifies timeouts.
- Model-facing `test` tool is registered.
- Compact reports are useful for common Rust/Python failures.
```

## Files likely to modify

Core events:

```text
crates/codegg-core/src/bus/events.rs
src/core event mapping files, if AppEvent-to-CoreEvent mapping is maintained separately
crates/codegg-protocol, if CoreEvent protocol variants are mirrored there
```

TUI command plumbing:

```text
src/tui/command.rs
src/tui/app/mod.rs or nearby command handling modules
src/tui runtime/task handling modules as applicable
```

Test runner/tool integration:

```text
src/test_runner/runner.rs
src/tool/test.rs
```

Documentation:

```text
architecture/command.md
architecture/tool.md
```

Use actual file locations discovered in the repo. The architecture docs indicate command execution lives around the TUI command registry and app command handling, but implementation names may have drifted.

## Event model

Add minimal lifecycle events to `AppEvent`:

```rust
TestRunStarted {
    session_id: String,
    job_id: String,
    command: String,
    cwd: String,
}

TestRunProgress {
    session_id: String,
    job_id: String,
    message: String,
}

TestRunCompleted {
    session_id: String,
    job_id: String,
    status: String,
    summary: String,
    log_dir: Option<String>,
}
```

Keep payloads compact. Do not stream raw test output through the app bus. The full output already belongs in `.codegg/test-runs/`. Progress messages should be low-cardinality status updates such as:

```text
started cargo test -p codegg-core
running 42 tests
last progress: test foo::bar ... ok
failed: 1 primary failure
completed in 18.2s
```

Add event type names in `AppEvent::event_type()`:

```text
test_run:started
test_run:progress
test_run:completed
```

If the daemon bridge maps `AppEvent` to protocol-level `CoreEvent`, add corresponding compact core events. If doing so requires broad protocol churn, it is acceptable in this phase to keep events local to the TUI/app bus and document the daemon/protocol mapping as deferred.

## Runner event hook

Do not hard-wire the runner to global event state if avoidable. Add an optional callback/sink to the runner request or runner options:

```rust
pub trait TestEventSink: Send + Sync {
    fn started(&self, snapshot: TestRunStartedSnapshot);
    fn progress(&self, snapshot: TestRunProgressSnapshot);
    fn completed(&self, snapshot: TestRunCompletedSnapshot);
}
```

For minimal implementation, a boxed closure or mpsc sender is also acceptable. The important constraint is that parser/runner logic should remain testable without a global TUI.

`TestTool` can pass an event sink that publishes `AppEvent` when a session id is available through `ToolExecutionContext`. If session id is unavailable, the tool should still work without event publishing.

## Progress throttling

Do not publish an event for every output line. Add throttling:

```text
publish started immediately
publish progress on meaningful parser state changes
publish progress at most once every N milliseconds, e.g. 500-1000ms
publish completed immediately
```

Meaningful changes:

```text
test count discovered
pass/fail count changed
failure detected
timeout warning if implemented
process completed
```

This avoids turning the bus into another high-volume raw-output stream.

## `/test` slash command

Add built-in slash commands:

```text
/test
/test workspace
/test changed
/test package <name>
/test file <path>
/test previous
/test custom <command>
```

The minimal command parser can be simple:

```text
no args -> auto
workspace -> workspace
changed -> changed
package NAME -> package
file PATH -> file
previous -> previous_failures
custom REST -> custom, subject to the same restrictions as TestTool
```

For safety and consistency, `/test custom` should use the same command validation as the model-facing tool. Do not create a more permissive custom execution path by accident.

## TUI behavior

When a user invokes `/test`, run the supervised test runner outside the LLM path and display a concise result in the conversation/status area. The command should not automatically send the report to the LLM unless the user explicitly asks for agent follow-up.

Recommended minimal behavior:

```text
1. User enters /test package codegg-core.
2. TUI starts supervised run and shows status/progress events.
3. Completion posts a compact report to the session UI.
4. Full log path is visible in the report.
```

If codegg has a command-channel/background-task mechanism already, reuse it. If not, spawn a bounded tokio task and keep cancellation deferred unless the TUI already has a cancellation control pattern.

## Optional cancellation hook

If there is an existing command/task cancellation mechanism, wire `/test` jobs into it. If not, do not invent a large job-control system in this phase.

Document deferred cancellation support as:

```text
/test jobs currently run to completion or runner timeout. Interactive cancellation should be added when test jobs become first-class background jobs.
```

The runner itself should already kill on timeout from Phase 2.

## Relationship to agent follow-up

Do not use `AgentLoop::follow_up_sender` as the primary completion mechanism for now. Existing follow-up semantics may not consume prompts queued after a run has returned unless another run occurs. Fully automatic agent resumption after test completion is a later milestone.

Manual TUI path should remain user-visible and deterministic:

```text
/test runs tests
report appears
user decides whether to ask agent to fix failures
```

## Documentation updates

Update `architecture/command.md` with `/test` in the built-in command list and command execution notes.

Update `architecture/tool.md` to mention that the `test` tool can publish lifecycle events when session context is available.

If there is user-facing README command documentation, add a concise entry:

```text
/test: Run supervised project tests with compact reporting and full logs under `.codegg/test-runs/`.
```

## Tests to add

Event tests:

```text
test_run_started_event_has_event_type
test_run_progress_event_has_event_type
test_run_completed_event_has_event_type
runner_publishes_started_and_completed_when_sink_present
runner_does_not_require_sink
progress_events_are_throttled
```

Command parser tests:

```text
slash_test_no_args_maps_to_auto
slash_test_workspace_maps_to_workspace
slash_test_changed_maps_to_changed
slash_test_package_requires_name
slash_test_file_requires_path
slash_test_previous_maps_to_previous_failures
slash_test_custom_uses_same_validation_as_tool
```

TUI integration tests if the existing harness supports them:

```text
slash_test_command_is_registered
slash_test_command_does_not_trigger_llm_template_path
slash_test_completion_renders_compact_report
```

## Acceptance criteria

This phase is complete when:

```text
- Test lifecycle events exist and have stable event type names.
- The runner/tool can publish started/progress/completed events when a session sink is available.
- Progress event volume is throttled and does not include raw full output.
- `/test` is registered as a built-in command.
- `/test` can run auto/workspace/changed/package/file/previous/custom scopes where supported.
- `/test` returns the same compact report style as the model-facing tool.
- `/test` does not automatically force an LLM turn.
- Documentation reflects the new command and event behavior.
```

## Validation

Run:

```text
cargo fmt
targeted event tests
targeted slash-command parser tests
targeted test_runner tests
cargo check
```

If TUI integration tests are expensive or brittle, keep this phase focused on command registration and parsing plus lower-level event behavior.

## Handoff notes

This phase is intentionally a visibility and ergonomics pass, not the final background-job architecture. The durable design point is that test output remains outside model context by default, while users and frontends get enough lifecycle signal to understand what is happening.
