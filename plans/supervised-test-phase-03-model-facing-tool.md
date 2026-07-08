# Supervised Test Subsystem Phase 3: Model-Facing `test` Tool

## Objective

Expose the supervised test runner to the model as a first-class native `test` tool. The tool should let the agent request common Rust/Python test runs without using `bash`, while codegg handles command resolution, process supervision, timeout classification, full-log retention, and compact report formatting.

This phase should still keep execution synchronous from the model's perspective: the tool call completes with a compact report. Fully detached background jobs and agent resumption are deferred.

## Precondition

Phase 1 should provide internal types, resolver, parser skeleton, and report formatter.

Phase 2 should provide a streaming supervised runner that returns `TestReport` and stores full logs under `.codegg/test-runs/`.

## Files to add or modify

Add:

```text
src/tool/test.rs
```

Modify:

```text
src/tool/mod.rs
src/permission/mod.rs if tool category mapping requires explicit name handling
architecture/tool.md if architecture docs are maintained in this pass
```

Optional tests:

```text
tests/test_tool_supervised.rs
```

## Tool contract

Implement a `TestTool` that implements the existing `Tool` trait.

Tool name:

```text
test
```

Tool description should be direct and model-facing:

```text
Run project tests through codegg's supervised test runner. Prefer this over bash for cargo test, cargo nextest, pytest, uv run pytest, go test, make test, and similar test commands. The tool streams full stdout/stderr to logs, classifies timeouts/failures, and returns a compact report instead of dumping full output into context.
```

Do not overpromise language support. If Phase 3 only supports Rust/Python/custom, say so.

## JSON schema

Use a minimal schema:

```json
{
  "type": "object",
  "properties": {
    "scope": {
      "type": "string",
      "enum": ["auto", "workspace", "changed", "package", "file", "previous_failures", "custom"],
      "description": "Test scope to run. Use auto for the default project test command."
    },
    "package": {
      "type": "string",
      "description": "Package/crate name for package scope."
    },
    "path": {
      "type": "string",
      "description": "File path for file scope."
    },
    "command": {
      "type": "string",
      "description": "Custom test command. Requires custom scope and must pass test command safety checks."
    },
    "workdir": {
      "type": "string",
      "description": "Working directory. Defaults to current working directory."
    },
    "timeout": {
      "type": "number",
      "description": "Wall-clock timeout in seconds. Default 300."
    },
    "stall_timeout": {
      "type": "number",
      "description": "No-output timeout in seconds. Default 120; set 0 to disable."
    }
  },
  "required": ["scope"]
}
```

If compatibility with model tool schemas prefers fewer enums, keep `scope` as string and validate manually.

## Scope parsing

Map JSON input to `TestScope`:

```text
scope=auto -> TestScope::Auto
scope=workspace -> TestScope::Workspace
scope=changed -> TestScope::Changed
scope=package -> require package
scope=file -> require path
scope=previous_failures -> TestScope::PreviousFailures
scope=custom -> require command
```

Canonicalize workdir if possible. Relative file paths should be interpreted relative to workdir.

Invalid requests should return `ToolError::Execution` with a clear message. Do not panic on malformed JSON.

## Permission and safety posture

Classify the tool conservatively. Recommended first pass:

```rust
fn category(&self) -> ToolCategory {
    ToolCategory::ShellExec
}
```

Rationale: tests execute arbitrary project code and may mutate files, use network, consume resources, or run build scripts. Do not classify `test` as read-only.

If `permission/mod.rs` maps tool names explicitly, add `test` there with shell-exec semantics. If unknown tools already fall back to mutating, still add explicit mapping for clarity.

Custom command handling must not create a new unreviewed command execution bypass. For Phase 3, use one of these approaches:

```text
Preferred: disable custom scope for model-facing use until shared shell-policy validation exists.
Acceptable: allow custom only when it matches a strict test-command prefix allowlist.
Do not: accept arbitrary command strings as model-facing test commands without permission/security checks.
```

Strict prefixes may include only test-oriented commands already recognized elsewhere:

```text
cargo test
cargo nextest
pytest
uv run pytest
go test
zig build test
make test
make check
npm test
pnpm test
yarn test
bun test
```

Keep this list local only if a later cleanup will share it with agent-loop test-command detection. Prefer extracting a common helper if the diff is small.

## Tool execution flow

`TestTool::execute` should:

```text
1. Parse and validate JSON input.
2. Resolve workdir.
3. Build TestRunRequest.
4. Call resolve_and_run_test or resolve + run_resolved_test.
5. Format TestReport with report formatter.
6. Return compact text.
```

If resolution fails, return a concise error explaining how the model/user can make scope explicit.

If the runner returns a report with Failed or TimedOut status, the tool call itself should still be considered successful from the tool infrastructure perspective. A failing test is not a tool failure. The returned text should say tests failed/timed out.

Only infrastructure failures should become `ToolError`, for example:

```text
could not create log directory
could not spawn resolved command
invalid request
permission/security rejection
```

## Structured execution

Consider overriding `execute_structured` so provenance reflects native execution. This is optional in the first pass because the default wrapper already produces legacy structured provenance, but the better implementation should attach:

```text
backend: native
implementation: test_runner
trust: local
elapsed_ms
```

If existing `StructuredToolResult` helpers make this easy, implement it now. Otherwise, defer and rely on the default `execute_structured` behavior.

## Registry integration

Modify `src/tool/mod.rs`:

```text
pub mod test;
```

Register near shell/filesystem tools in `ToolRegistry::with_options`:

```text
registry.register(crate::tool::test::TestTool::default());
```

Ensure it appears in model-facing definitions unless the feature is explicitly disabled in config. No config gate is required for the minimal phase unless maintainers prefer a hidden/experimental flag.

## Tool guidance updates

Update the `bash` tool description or model profile guidance if appropriate so agents prefer `test` for test commands. Keep this concise; the tool description itself should do most of the work.

If there is a built-in skill or prompt file for agent tool usage, add a note:

```text
Use `test` rather than `bash` for normal test execution so codegg can supervise long-running tests, capture full logs, and return compact failure reports.
```

Do not remove `bash`; test execution through shell remains useful for manual or unusual workflows.

## Tests to add

Add unit/integration tests for `TestTool`:

```text
test_tool_rejects_missing_scope
test_tool_rejects_package_scope_without_package
test_tool_rejects_file_scope_without_path
test_tool_rejects_custom_scope_without_command
test_tool_formats_success_report
test_tool_formats_failure_report_without_marking_tool_error
test_tool_formats_timeout_report_without_marking_tool_error
test_tool_registered_in_default_registry
test_tool_category_is_shell_exec_or_expected_conservative_category
```

For tests that need process execution, use the runner's test helpers or small temp projects. Avoid invoking the full repository test suite from a unit test.

## Documentation updates

Update `architecture/tool.md` built-in tool list if this repository keeps that file synchronized with tool registration.

Add a short entry under shell execution or a new test execution section:

```text
`test`: Runs project tests through the supervised test runner. Streams logs to `.codegg/test-runs`, classifies common Rust/Python failures and timeouts, and returns compact reports to the model.
```

If built-in tool counts are asserted in tests or docs, update the count.

## Acceptance criteria

This phase is complete when:

```text
- `test` is registered in the default tool registry.
- The model-facing schema supports auto/workspace/changed/package/file/custom scopes.
- The tool returns compact reports for pass/fail/timeout.
- Failing tests do not surface as infrastructure tool errors.
- Timeout reports include timeout kind and log path.
- Custom commands are disabled or tightly allowlisted.
- The tool category is conservative and permission-aware.
- Existing `bash` and `terminal` behavior remains unchanged.
```

## Validation

Run:

```text
cargo fmt
targeted test tool tests
targeted test_runner tests
cargo check
```

If full workspace tests are too heavy, do not force them in this phase. The point is to validate the new tool path without reintroducing the heavy test overhead this subsystem is designed to reduce.

## Handoff notes

The most important semantic rule is that test failures are data, not tool failures. The agent must get a compact report and continue reasoning from it. Reserve `ToolError` for cases where codegg could not perform the supervised execution at all.
