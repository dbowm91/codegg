# Supervised Test Phase 07: Protocol, Daemon, and Remote Frontend Integration

## Context

The supervised test subsystem currently has a strong local vertical slice:

```text
- model-facing `test` tool
- deterministic `src/test_runner` resolver/parser/runner/reporter
- strict custom command validation
- full logs under `.codegg/test-runs/`
- compact model-facing reports
- `/test` TUI command
- AppEvent lifecycle events: started/progress/completed
- stale local TUI completion protection
```

The remaining architecture gap is frontend reach. Local TUI paths can see test lifecycle events, but remote daemon/frontends need a stable protocol-level representation if codegg is to keep the core/daemon/frontend split clean. This phase makes supervised test events first-class over the core protocol while preserving the current deterministic execution contract.

This is not a fully detached test job system. The goal is protocol visibility and remote control surface preparation, not persistent background job orchestration.

## Primary objective

Expose supervised test lifecycle information through the daemon/protocol boundary so all frontends can observe test runs consistently.

The phase should provide:

```text
- protocol-level test run event types
- AppEvent -> CoreEvent bridge mapping
- stable serialized payloads for started/progress/completed
- remote frontend-compatible compact event data
- optional read-only test-run listing/detail APIs if existing protocol patterns make this cheap
- documentation for local vs remote behavior
```

## Non-goals

Do not implement:

```text
- persistent daemon-owned test jobs across restarts
- automatic model/agent resumption after test completion
- remote cancellation unless an existing generic command cancellation path makes it trivial
- streamed raw stdout/stderr over protocol
- log file download APIs unless file artifact infrastructure already supports them
- RTK test memory
- semantic failure clustering
```

The remote frontend should receive compact status/progress/completion information. Full raw output remains file-backed under `.codegg/test-runs/`.

## Current architecture surfaces to inspect

Inspect these areas before patching:

```text
crates/codegg-core/src/bus/events.rs
src/core/daemon.rs
crates/codegg-protocol/src/...
src/tui/runtime/app_events.rs
src/tui/runtime/command_dispatch.rs
src/tui/app/mod.rs
src/tui/commands/test.rs
```

The previous implementation added `AppEvent::TestRunStarted`, `AppEvent::TestRunProgress`, and `AppEvent::TestRunCompleted`. This phase should preserve those and bridge them to protocol/core events rather than inventing a parallel local-only event stream.

## Event model

### Recommended protocol event variants

Add protocol/core equivalents to the existing AppEvent lifecycle:

```rust
pub enum CoreEvent {
    // existing variants ...
    TestRunStarted(TestRunStartedEvent),
    TestRunProgress(TestRunProgressEvent),
    TestRunCompleted(TestRunCompletedEvent),
}
```

or, if the protocol uses tagged structs rather than enum payload variants, follow the existing style.

Payloads should be compact and stable:

```rust
pub struct TestRunStartedEvent {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub job_id: String,
    pub command: String,
    pub cwd: String,
    pub scope_label: Option<String>,
    pub started_at: Option<String>,
}

pub struct TestRunProgressEvent {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub job_id: String,
    pub message: String,
    pub tests_seen: Option<u64>,
    pub tests_passed: Option<u64>,
    pub tests_failed: Option<u64>,
    pub elapsed_ms: Option<u64>,
}

pub struct TestRunCompletedEvent {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub job_id: String,
    pub status: String,
    pub summary: String,
    pub failure_class: Option<String>,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub log_dir: Option<String>,
    pub report_json: Option<String>,
}
```

If the existing `AppEvent` payload does not yet include all optional fields, keep the protocol fields optional and populate only what is already available. Do not broaden the runner report/event plumbing excessively in this phase.

### Serialization contract

Event type names should remain stable and align with the existing AppEvent event types:

```text
test_run:started
test_run:progress
test_run:completed
```

If the protocol uses snake_case variant names, map consistently:

```text
testRunStarted / test_run_started / test_run:started
```

Use the existing protocol naming convention rather than introducing a new style.

### Raw output rule

Never include raw stdout/stderr lines in protocol progress events.

Allowed data:

```text
- compact progress message
- aggregate counts
- elapsed time
- status
- summary
- log path metadata
```

Disallowed data:

```text
- unbounded stdout chunks
- unbounded stderr chunks
- full failure messages beyond the already bounded report summary
- entire report.json body in an event
```

## AppEvent to CoreEvent bridge

### Tasks

Update the daemon bridge in `src/core/daemon.rs` or equivalent mapping function.

Current behavior likely has a `bridge_app_event`-style function that maps app events to core/protocol events. Extend it to include:

```text
AppEvent::TestRunStarted -> CoreEvent::TestRunStarted
AppEvent::TestRunProgress -> CoreEvent::TestRunProgress
AppEvent::TestRunCompleted -> CoreEvent::TestRunCompleted
```

Preserve existing turn/session behavior. If daemon bridge currently attaches active turn IDs to events, keep that pattern.

### Missing session IDs

The runner and tool paths may sometimes produce empty session IDs when a sink exists outside a session-aware context. Do not panic. Protocol mapping should either:

```text
- forward the empty/unknown session ID if that is current behavior elsewhere, or
- drop the event with a debug log if protocol requires a non-empty session ID
```

Prefer consistency with existing event bridge behavior.

### Acceptance criteria

```text
- Test AppEvents are bridged into protocol/core events.
- Mapping preserves session_id and active turn_id behavior.
- Unknown/missing session IDs do not panic.
- No raw output crosses the bridge.
```

## Protocol definitions

### Tasks

Inspect `crates/codegg-protocol` for existing event definition patterns.

Add test event payloads in the same crate/location as other core events. Ensure:

```text
- serde derives are present
- field names are stable
- optional fields are used for data not guaranteed by current runner events
- backwards compatibility is preserved if clients ignore unknown event variants
```

If protocol versioning exists, bump or document the addition according to existing convention. If there is no explicit protocol version, add release-note/docs wording only.

### Acceptance criteria

```text
- Protocol crate compiles.
- New event variants serialize/deserialize in unit tests if comparable tests exist.
- Existing protocol tests remain unchanged or updated only for additive variants.
```

## Frontend behavior

### Local TUI

The local TUI already has direct `/test` command behavior. Do not replace it with remote protocol plumbing.

For local TUI, this phase should only ensure:

```text
- no duplicate test progress display appears after CoreEvent mapping
- existing `/test` command remains non-LLM
- stale request protection remains intact
```

### Remote/web/mobile/frontends

For remote clients consuming the core protocol, expected behavior is:

```text
- receive test_run:started when a supervised test begins
- receive throttled test_run:progress events
- receive test_run:completed with compact status and log metadata
- decide their own UI rendering
```

No remote frontend must be fully implemented in this phase unless a skeleton or existing test client needs a variant update to compile.

### Acceptance criteria

```text
- Local TUI behavior is unchanged.
- Remote protocol clients can observe test events.
- Unknown/new event variants are handled according to existing client conventions.
```

## Optional read-only test-run metadata APIs

This is optional. Implement only if the existing protocol already has a clean request/response pattern for read-only session artifacts.

Potential APIs:

```text
ListTestRuns { session_id, limit }
GetTestRunReport { session_id, run_id }
```

Responses should read from:

```text
.codegg/test-runs/index.json
.codegg/test-runs/<run-id>/report.json
```

They should not stream raw stdout/stderr in this phase.

If implemented, keep them read-only and bounded:

```text
- list returns index metadata only
- get report returns bounded report JSON or compact report text
- missing files return clear errors
```

If not implemented, document as deferred.

## Cancellation and remote control

Do not add remote test cancellation unless codegg already has a generic async command cancellation API that can be reused safely.

If cancellation is deferred, document:

```text
- local TUI may or may not have cancellation depending on existing command infrastructure
- remote clients can observe but not cancel supervised tests in this phase
- future persistent job registry should own cancellation semantics
```

## Tests

### Core/protocol tests

Add tests consistent with existing style:

```text
protocol_serializes_test_run_started
protocol_serializes_test_run_progress
protocol_serializes_test_run_completed
protocol_deserializes_test_run_completed_with_optional_fields_missing
```

### Daemon bridge tests

If bridge mapping is testable:

```text
bridge_maps_test_run_started
bridge_maps_test_run_progress
bridge_maps_test_run_completed
bridge_does_not_include_raw_output
bridge_preserves_active_turn_id_if_available
```

If bridge mapping is not currently easy to test, extract a pure function for AppEvent -> CoreEvent conversion and test that function.

### TUI regression tests

Add or confirm:

```text
local_test_command_still_uses_direct_tui_path
stale_test_completion_still_ignored
no_duplicate_display_on_test_event_bridge
```

If duplicate-display cannot be tested directly, document manual validation steps.

### Optional API tests

If list/get APIs are added:

```text
list_test_runs_reads_bounded_index
get_test_run_report_reads_report_json
get_test_run_report_missing_file_returns_error
```

## Documentation updates

Update:

```text
architecture/test_runner.md
architecture/command.md
architecture/overview.md
architecture/core-daemon or equivalent daemon/protocol doc
crates/codegg-protocol docs if present
README.md only if user-facing remote frontend behavior is documented there
AGENTS.md validation commands
```

Docs should state:

```text
- AppEvent lifecycle events are now bridged to protocol/core events
- remote frontends receive compact started/progress/completed events
- raw stdout/stderr remains file-backed and is not streamed over protocol
- local `/test` remains deterministic and non-LLM
- remote cancellation is deferred unless implemented
- optional list/get metadata APIs are available or explicitly deferred
```

## Validation commands

Run targeted validation:

```text
cargo fmt --check
cargo check
cargo test -p codegg-core --lib
cargo test -p codegg-protocol --lib
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib tui::commands::test
```

If the exact package names differ, adjust to the workspace. Keep this targeted; do not force a full workspace test run if it violates the repo's resource constraints.

## Implementation order

1. Inspect existing protocol/core event definitions.
2. Add protocol event payloads using existing naming/serde conventions.
3. Add AppEvent -> CoreEvent bridge mappings.
4. Add serialization and bridge tests.
5. Confirm local TUI behavior remains unchanged.
6. Optionally add read-only list/get test-run metadata APIs if trivial.
7. Update docs and validation commands.
8. Run targeted validation.

## Final acceptance checklist

This phase is complete when:

```text
- protocol/core event types exist for supervised test started/progress/completed
- AppEvent test lifecycle events bridge to CoreEvent/protocol events
- remote frontends can observe compact test lifecycle events
- raw stdout/stderr is not sent through protocol lifecycle events
- local `/test` behavior remains unchanged and non-LLM
- stale completion protection remains intact
- optional list/get APIs are implemented or explicitly deferred
- docs describe local and remote behavior accurately
- targeted validation passes
```

## Deferred follow-up after this phase

After phase 07, the supervised test line should be complete for the current architecture track. Future work should be explicitly feature-scoped:

```text
- persistent daemon-managed test job registry
- remote cancellation and job control
- RTK/context-artifact test memory
- previous-failure garbage collection and log retention policy
- nextest/JUnit/pytest structured-output adapters
- model-side automatic resumption policies
```

Do not implement those as incidental additions to this phase.
