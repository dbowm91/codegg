# Phase 7: Test Matrix and Parity Validation

## Goal

Build the validation matrix that proves Codegg's eggsearch and eggsact integration is correct, bounded, diagnosable, and regression-resistant.

This phase should run after the major wrapper and config work is implemented. It is the gate before deprecating duplicate legacy paths or making expanded tools part of the default release posture.

## Test categories

### 1. Eggsearch adapter unit tests

Cover each adapter function in `src/search_backend/eggsearch.rs`.

Required cases:

- Valid Codegg-native input maps to expected eggsearch MCP arguments.
- Required argument missing returns a clear error.
- Empty query/URL/path is rejected.
- `max_results` and `num_results` aliases normalize correctly where applicable.
- Provider hints normalize correctly for search-like tools.
- Output is clamped at the configured cap.
- Trust frame is present and closed.
- Timeout becomes `ToolError::Timeout`.
- MCP failure becomes actionable `ToolError::Execution`.
- Missing upstream MCP tool includes the upstream tool name and discovered tool list when available.

Prefer table-driven tests to keep coverage readable.

### 2. Eggsearch bootstrap tests

Cover startup and state behavior.

Required cases:

- `[search].backend = "eggsearch"` attempts to connect.
- Explicit `[mcp.eggsearch]` block is honored.
- Missing eggsearch is non-fatal at startup.
- `backend = "builtin"` does not spawn eggsearch.
- `backend = "disabled"` does not spawn eggsearch and tools return disabled errors.
- Re-entry does not create inconsistent state.
- Bootstrap report includes backend, command, server name, tools, caps, fallback, raw exposure, and connection status.

### 3. Raw MCP exposure tests

Cover model-facing definitions.

Required cases:

- `expose_raw_mcp_tools = false` hides `mcp__eggsearch__...` tools.
- `expose_raw_mcp_tools = true` exposes raw tools for expert/debug use.
- Native Codegg wrappers remain registered in both cases.
- Raw MCP hiding works for all expanded eggsearch tools, not only `web_search` and `web_fetch`.

### 4. Eggsact adapter unit tests

Cover the in-process adapter from Phase 3.

Required cases:

- Adapter initializes with configured model profile.
- Adapter initializes with configured harness profile.
- Unknown profile is rejected or falls back with a warning according to policy.
- `text_equal` succeeds and returns deterministic output.
- `validate_json` succeeds/fails deterministically.
- Unknown tool returns a Codegg error.
- Tool unavailable in profile returns a Codegg error.
- Tool not allowed for model audience is rejected.
- Harness audience can execute harness-only preflight where expected.
- Budget or output limits are applied.
- Cancellation flag is passed through if supported.
- Provenance marks backend `native` and implementation `eggsact/<tool>`.

### 5. Eggsact model-facing registry tests

Cover `ToolRegistry` behavior.

Required cases:

- Default definitions include only approved model-facing eggsact wrappers.
- Deferred eggsact tools do not appear in default definitions.
- Deferred eggsact tools are discoverable through `tool_search` or catalog search.
- Disabled deterministic backend hides eggsact wrappers.
- Expert tools require explicit config.
- Tool categories are read-only for pure validators.
- Wrapper descriptions do not imply mutation or shell execution.

### 6. Harness preflight tests

Cover automatic internal checks.

Required cases:

- Patch with no matching target blocks before mutation.
- Patch with ambiguous replacement blocks or warns according to policy.
- Config write with invalid JSON/TOML warns or blocks according to policy.
- Valid config write passes.
- Shell command preflight warning is surfaced in permission/tool output path.
- Dangerous shell command still goes through existing Codegg permission checks.
- Unicode/identifier finding is warn-only by default.
- Observe mode records findings but does not alter operation outcome.
- Harness preflight events do not appear as separate model tool calls.

### 7. Integration tests with mock eggsearch

Use a small fake MCP server or existing MCP test support to simulate eggsearch.

Required cases:

- Each expanded wrapper calls the expected upstream tool.
- Server advertises only a subset of tools; wrappers for missing tools fail clearly.
- Server returns oversized output; Codegg clamps and marks truncation.
- Server returns malformed payload; Codegg does not panic.
- Provider status fails; doctor output remains useful.

Do not require live network access for default CI.

### 8. Optional live smoke tests

Add feature-gated or ignored tests for real eggsearch and real provider behavior.

Suggested feature:

```toml
[features]
live-eggsearch-tests = []
```

Live tests should require explicit environment variables and be skipped by default. They can verify:

- `eggsearch mcp stdio` starts.
- `provider_status` responds.
- `web_search` returns a bounded response with a configured provider.
- `repo_search` and `security_search` respond when providers are configured.

### 9. Golden output tests

For deterministic eggsact wrappers and framing helpers, add golden tests where useful.

Golden tests should cover:

- Trust frame shape.
- Deterministic output envelope.
- Provenance serialization.
- Doctor summary fragments.

Avoid overly brittle tests for elapsed time or provider result order.

### 10. Regression tests for legacy behavior

Verify existing behavior remains stable:

- `websearch` with builtin backend still works or returns the existing clear provider error.
- `webfetch` builtin path still works for valid input.
- Existing `security`, `research`, `bash`, `apply_patch`, `replace`, and `edit` tests continue to pass.
- Tool count changes are intentional and documented.

## CI expectations

The normal validation gate should include:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

If all-features pulls live network behavior, split live smoke tests behind ignored tests or a separate feature not included by default CI.

## Acceptance criteria

- Every new wrapper has unit tests and at least one integration-path test.
- Eggsearch can be absent without crashing Codegg startup.
- Eggsact can be disabled by config without breaking registry construction.
- Model-facing tool definitions are tested for bloat and raw MCP leakage.
- Harness preflight behavior is covered in observe/warn/block modes.
- CI remains network-independent by default.

## Risks

The main risk is test flakiness from async MCP and live provider behavior. Keep default tests mocked and deterministic. Treat live tests as explicit smoke checks, not release-blocking unit tests.
