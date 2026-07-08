# Supervised Test Subsystem Hardening and Validation Plan

## Context

The supervised test subsystem has landed as a five-phase vertical slice after the roadmap:

```text
Phase 1: src/test_runner core types, resolver, parser skeleton, formatter
Phase 2: streaming runner, log capture, wall/no-output timeout handling
Phase 3: model-facing `test` tool with ShellExec category and compact reports
Phase 4: failure-class taxonomy and improved Rust/pytest report quality
Phase 5: AppEvent lifecycle events and `/test` TUI command
```

This plan is a hardening and validation pass. It should not expand scope into RTK-backed test memory, coverage-aware selection, or fully detached background jobs. The goal is to prove the current minimal subsystem is safe, deterministic, portable enough, and integrated correctly before additional features are layered on.

## Primary goals

1. Confirm the current implementation compiles and passes the relevant targeted suites.
2. Validate the model-facing `test` tool behavior end-to-end for pass, failure, timeout, resolver error, and custom-command rejection.
3. Validate `/test` TUI command behavior without forcing an LLM turn.
4. Harden child-process cleanup and timeout handling enough to avoid leaked long-running test children.
5. Audit custom command validation so `test` does not become a shell-execution bypass.
6. Confirm AppEvent lifecycle events do not flood the bus and do not leak raw logs.
7. Identify daemon/protocol mapping gaps explicitly without blocking the minimal local TUI path.
8. Ensure documentation and architecture files match the implemented behavior.

## Non-goals

Do not implement these in this pass unless a small bug fix is required to keep existing behavior correct:

```text
- RTK-backed test history or failure memory
- test impact analysis from changed files
- coverage-aware test selection
- nextest JSON/JUnit parsing
- pytest plugin integration
- automatic rerouting from bash test commands into test tool
- persistent daemon-managed test jobs across restarts
- automatic agent resumption after a detached test job completes
```

## Track A: Baseline compile and targeted validation

### Tasks

Run and record the minimum validation matrix already referenced by the implementation commits and docs:

```text
cargo fmt --check
cargo check
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib tool::test
cargo test -p codegg --lib test_runner::runner::tests
cargo test -p codegg --lib tui::commands::test
```

If the repo's normal resource constraints make broad `cargo test` too heavy, keep this pass targeted. Do not reintroduce the test overhead this feature is intended to mitigate.

### Follow-up checks

Also run focused compile checks for TUI and core/event surfaces if not covered by `cargo check`:

```text
cargo check -p codegg
cargo test -p codegg-core --lib bus::events
```

If exact package/module targets differ, adjust commands to the workspace layout.

### Acceptance criteria

```text
- Formatting passes.
- Workspace or package compile check passes.
- test_runner targeted tests pass.
- model-facing test tool tests pass.
- TUI command parser tests pass.
- Event type tests pass.
- Any failing test is classified as unrelated, flaky, or a real regression with a linked follow-up item.
```

## Track B: Process supervision and cleanup hardening

### Rationale

Phase 2 intentionally used a minimal child-process cleanup model. That is acceptable for initial implementation, but a validation pass must confirm timeouts do not leave child processes or stream-reader tasks behind.

### Tasks

Inspect `src/test_runner/runner.rs` for:

```text
- child.kill().await or equivalent on wall-clock timeout
- child.kill().await or equivalent on no-output timeout
- bounded wait after kill
- stdout/stderr reader task cancellation or completion behavior
- behavior when stdout/stderr tasks finish before child exits
- behavior when child exits before stream tasks finish
- behavior when report.json write fails
- behavior when log directory creation fails
```

Add or improve tests:

```text
runner_timeout_kills_child
runner_timeout_does_not_hang_waiting_for_stream_tasks
runner_stall_timeout_kills_child
runner_spawn_error_does_not_create_partial_success_report
runner_report_json_write_failure_is_nonfatal_or_explicit
runner_log_dir_failure_returns_error_before_spawn
```

Use short-running deterministic helper commands. If platform-specific commands are required, guard tests with `cfg(unix)` and document Windows follow-up separately.

### Process group notes

If process-group cleanup is not implemented, add a clearly documented TODO and a test note. The hardening pass should prefer a small Unix implementation if feasible:

```text
- set child into a separate process group before exec on Unix
- kill process group on timeout
- fall back to child.kill() on unsupported platforms
```

Do not block the pass on full cross-platform job-object cleanup, but explicitly document the remaining gap.

### Acceptance criteria

```text
- Timeout paths are deterministic and bounded.
- Timeout paths attempt cleanup before returning a report.
- Reader tasks cannot indefinitely block completion after timeout.
- Process-group cleanup gap is either fixed on Unix or documented as an explicit limitation.
- Tests cover both wall-clock and no-output cleanup behavior.
```

## Track C: Security audit of custom command handling

### Rationale

The `test` tool is categorized as `ShellExec`, which is correct. The remaining risk is the `custom` scope. It must not become a lower-friction generic command execution path than `bash`.

### Tasks

Inspect `src/tool/test.rs` and `src/tui/commands/test.rs` for custom command validation.

Confirm:

```text
- model-facing custom commands are disabled or strictly allowlisted
- `/test custom` uses exactly the same validation as the model-facing tool
- allowlist recognizes only test-oriented command prefixes
- allowlist matching does not allow prefix smuggling such as `cargo test; ...`
- allowlist matching does not allow wrapper commands that bypass policy unless explicitly intended
- errors are clear and do not suggest unsafe bypasses
```

Add tests:

```text
test_tool_rejects_empty_custom_command
test_tool_rejects_non_test_custom_command
test_tool_rejects_custom_command_with_shell_separator
test_tool_rejects_custom_command_with_command_substitution
test_tool_allows_cargo_test_custom_command
test_tool_allows_pytest_custom_command
tui_test_custom_reuses_tool_validation
tui_test_custom_rejects_unsafe_suffix_after_allowed_prefix
```

If command validation is currently duplicated, extract a shared helper:

```text
src/test_runner/custom.rs
or
src/tool/test.rs public(crate) validation helper reused by TUI command parser
```

Prefer a single source of truth to avoid drift.

### Acceptance criteria

```text
- `test` custom command handling is not more permissive than intended.
- TUI and model-facing custom validation are consistent.
- Separator/suffix smuggling cases are tested.
- The tool remains categorized as ShellExec.
```

## Track D: Model-facing tool behavior validation

### Tasks

Exercise the `test` tool through direct tool invocation tests using temporary Rust/Python fixtures. Avoid running the full codegg workspace.

Fixtures should cover:

```text
Rust pass:
  temporary Cargo project with one passing test

Rust assertion failure:
  temporary Cargo project with one failing test

Rust compile error:
  temporary Cargo project with invalid test code

Pytest pass:
  temporary Python project with one passing test if pytest is available

Pytest failure:
  temporary Python project with one failing test if pytest is available

Resolver ambiguity:
  temporary project containing both Cargo.toml and pytest markers

Unknown ecosystem:
  empty temp directory
```

When external tools like `pytest` are unavailable, mark tests as skipped rather than failing. Rust fixture tests should be stable in the normal Rust build environment.

Validate report semantics:

```text
- passing tests produce first line indicating pass
- failing tests produce tool success with failed report text
- timeout produces tool success with timeout report text
- resolver errors produce ToolError with actionable message
- full logs are written under .codegg/test-runs
- report text is bounded and does not include raw full logs
```

### Acceptance criteria

```text
- Test failures are not represented as tool infrastructure failures.
- Infrastructure failures are represented as ToolError.
- Log paths are present in reports.
- Report text is bounded and stable.
- Model-facing schema matches accepted arguments.
```

## Track E: Parser and report robustness

### Tasks

Review `src/test_runner/parse.rs` and `src/test_runner/report.rs` for fragile assumptions.

Add fixtures or unit tests for:

```text
Rust test name with spaces/special module paths
Rust panic message containing commas, colons, quotes, and backticks
Rust compile error followed by location line
Rust compile error without location line
Rust doctest failure line
Pytest assertion with multi-line message
Pytest ERROR during collection
Pytest ERROR for a specific test case
Pytest short summary line with long message
Non-UTF-8 or lossy output path through formatter
Very long failure message truncation on UTF-8 boundary
More than 5 failures with omitted-count note
```

Review failure-class ordering:

```text
rust_compile_error should outrank generic nonzero_exit
pytest_collection_error should not be flattened to pytest_failure
timeout classes should be explicit
unknown_failure should be used only when no better signal exists
```

### Acceptance criteria

```text
- Parser handles common Rust and pytest output variants without panics.
- Reports remain compact under large failure output.
- Failure-class summary is deterministic.
- Formatter never emits unbounded raw output.
```

## Track F: Event bus and TUI command validation

### Tasks

Review event publishing path:

```text
- TestRunStarted includes session_id, job_id, command, cwd
- TestRunProgress is throttled
- TestRunProgress never includes raw output chunks
- TestRunCompleted includes status, summary, optional log_dir
- event_type strings are stable
```

Review `/test` command path:

```text
- `/test` maps to auto scope
- `/test workspace` maps to workspace scope
- `/test changed` maps to changed scope
- `/test package <name>` requires name
- `/test file <path>` requires path
- `/test previous` handles unsupported previous-failures gracefully if no report index exists
- `/test custom <command>` uses shared custom validation
- `/test` does not create an LLM prompt/template turn
- completion displays compact report to the session UI/status area
```

Add tests if missing:

```text
test_command_does_not_trigger_agent_template_path
test_command_reports_resolver_error_to_ui
test_command_reports_timeout_to_ui
test_event_progress_is_throttled_under_many_lines
test_event_progress_does_not_include_raw_line_payloads
```

### Daemon/protocol mapping check

Inspect whether `AppEvent::TestRun*` is mapped into protocol-level `CoreEvent`. If not, document the gap in `architecture/test_runner.md` or `architecture/command.md`:

```text
Local TUI receives AppEvent lifecycle events. Remote daemon/frontends do not yet receive protocol-level test events unless CoreEvent mapping is added.
```

Do not add full protocol support unless it is a small, straightforward mapping.

### Acceptance criteria

```text
- `/test` works independently of LLM turn creation.
- Events are compact and throttled.
- Event visibility behavior is documented accurately for local and remote frontends.
- Unsupported `/test previous` behavior is clear and non-panicking.
```

## Track G: Resolver correctness and edge cases

### Tasks

Review `src/test_runner/resolve.rs` for common workspace edge cases:

```text
- mixed Rust/Python root ambiguity
- Rust workspace package scope
- Python file scope
- Rust file scope currently conservative fallback
- changed scope currently fallback behavior
- previous_failures unsupported behavior
- custom scope validation handoff
- workdir canonicalization
- non-existent path handling
```

Add tests:

```text
resolver_file_scope_rejects_missing_file
resolver_file_scope_python_uses_relative_path_from_workdir
resolver_package_scope_rust_preserves_package_name_as_arg
resolver_package_scope_rejects_empty_package
resolver_changed_scope_documents_fallback
resolver_previous_failures_returns_clear_unsupported_until_index_exists
resolver_auto_unknown_in_empty_dir_is_clear
resolver_auto_mixed_project_is_clear
```

### Acceptance criteria

```text
- Resolver errors are explicit and actionable.
- Resolver behavior matches docs for changed and previous_failures.
- No scope silently runs a broader command than documented without a clear scope_label/report note.
```

## Track H: Documentation and architecture consistency

### Tasks

Check and update:

```text
architecture/test_runner.md
architecture/tool.md
architecture/command.md
architecture/overview.md
README.md
AGENTS.md
```

Ensure docs state:

```text
- `test` is ShellExec, not read-only
- full logs are under `.codegg/test-runs/`
- model receives compact reports, not full logs
- `/test` does not automatically involve the LLM
- custom commands are allowlisted/restricted
- previous_failures and changed scope limitations are clear
- daemon/protocol event mapping status is clear
```

Correct any built-in tool count drift caused by adding `test`.

### Acceptance criteria

```text
- Docs match current behavior.
- Limitations are explicit.
- Validation commands in AGENTS.md are correct.
```

## Track I: Repository hygiene

### Tasks

Review incidental changes in the implementation range.

Specific item to inspect:

```text
src/theme/target.rs was restored/added in the Phase 3 commit as a pre-existing compilation fix. Confirm it is intentional, compiles, and is not masking an unrelated broken module boundary.
```

Check for generated test artifacts accidentally tracked or left in repo:

```text
.codegg/test-runs/
report.json
stdout.log
stderr.log
temporary fixture projects
```

Add `.gitignore` entries if needed:

```text
.codegg/test-runs/
```

Only add ignore rules if they do not conflict with intentional `.codegg` tracked config/assets.

### Acceptance criteria

```text
- No generated test logs are tracked.
- Incidental theme file is either validated or split into a separate cleanup commit.
- Git status after validation is clean except intentional changes.
```

## Recommended implementation order

1. Run Track A validation and record failures.
2. Fix compile/test breakages first.
3. Harden custom command validation before adding more tests around it.
4. Harden timeout/process cleanup.
5. Add parser/report edge-case tests.
6. Validate `/test` and event behavior.
7. Update docs with confirmed limitations.
8. Run targeted validation commands again.

## Final acceptance checklist

The pass is complete when all of the following are true:

```text
- cargo fmt --check passes.
- cargo check passes, or any failure is unrelated and documented.
- test_runner targeted tests pass.
- tool::test targeted tests pass.
- tui::commands::test targeted tests pass.
- timeout cleanup tests cover wall-clock and no-output paths.
- custom command validation rejects separator/suffix smuggling.
- model-facing reports remain bounded and distinguish failure vs infrastructure error.
- `/test` command path does not force an LLM turn.
- AppEvent lifecycle events are compact/throttled.
- protocol/daemon event mapping gap is either fixed or explicitly documented.
- docs match current behavior and limitations.
```

## Expected follow-up after this hardening pass

If this pass is clean, the next feature-level plan should be one of:

```text
- previous_failures report index and rerun support
- RTK/context-artifact backed test memory
- Unix process-group cleanup plus Windows job-object parity
- CoreEvent/protocol mapping for remote frontends
- optional nextest/JUnit/pytest structured-output adapters
```

Do not begin those until this hardening pass has closed the minimal subsystem.
