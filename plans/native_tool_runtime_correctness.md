# Native Tool Runtime Correctness Plan

## Purpose

Finish the hardening work for Codegg's library-first, MCP-second tool architecture.

The previous pass put most of the right pieces in place: config-to-runtime backend conversion, backend-aware registration for LSP/security, structured provenance methods on important tools, MCP raw-tool filtering, and `egggit` consumption from review/commit paths. This pass should focus narrowly on correctness: every runtime path must preserve backend config, diagnostics must describe actual behavior, and central tool execution must actually use the structured execution path.

Do not extract new tool crates in this pass.

## Current State Summary

The repo currently has:

```text
src/tool/backend.rs              backend/provenance/report types
src/tool/backend_config.rs       config schema -> runtime backend conversion
src/tool/disabled.rs             disabled placeholder tool
crates/egglsp                    LSP implementation crate
crates/egggit                    read-only git/worktree facts
crates/eggcontext                token estimation
crates/eggsec                    deterministic security scanning
```

Good progress already completed:

- `ToolBackendConfig::from_config(&Config)` exists.
- `ToolRegistry::with_config(&Config)` exists.
- `ToolRegistry` stores resolved `tool_backends`.
- LSP/security registration consults backend config.
- `McpService::list_filtered_tools(policy)` is used during tool definition construction.
- `websearch`, `webfetch`, `lsp`, and `security` have or are intended to have structured provenance.
- `review` and `commit` now use `egggit::diff_text()` for read-only diff acquisition.

Remaining risks:

- Some registry construction paths may still drop `[tool_backends]` and use `ToolBackendConfig::default()`.
- `execute_capture()` exists but may not be used by the central agent tool-call execution path.
- MCP fallback diagnostics can say fallback-native even though MCP-unimplemented registration currently installs a disabled placeholder.
- Disabled backend behavior may still expose disabled tools to the model, increasing tool-surface noise.
- Root dependency ownership needs one final audit.

## Non-Goals

Do not add MCP server binaries for `egglsp`, `eggsec`, `egggit`, or `eggcontext`.

Do not add new crates.

Do not rewrite the provider layer.

Do not change model-facing tool schemas unless required to fix incorrect disabled/MCP behavior.

Do not convert filesystem/edit/shell tools into external backends.

Do not expand `eggcontext` beyond token-estimation correctness in this pass.

## Phase 1: Find Every ToolRegistry Construction Path

Search for all registry constructors and confirm whether they preserve loaded config:

```bash
rg "ToolRegistry::with_defaults|ToolRegistry::with_config|ToolRegistry::with_session_defaults|ToolRegistry::with_options|ToolRegistryOptions" src tests crates
```

Classify each call site:

1. Static/default/test-only path where all-native default is correct.
2. Runtime path where loaded `Config` is available and must be used.
3. Session path where todo/session persistence is needed and loaded `Config` must still be preserved.

The goal is to eliminate accidental use of `ToolBackendConfig::default()` in real runtime/session paths.

Acceptance criteria:

- Every runtime registry construction path either calls `ToolRegistry::with_config(&config)` or passes `ToolBackendConfig::from_config(&config)` explicitly.
- Test-only/default call sites are documented or renamed to make all-native behavior explicit.
- No production path silently drops `[tool_backends]`.

## Phase 2: Replace or Fix `with_session_defaults()`

`with_session_defaults()` currently risks losing backend config because it constructs `ToolRegistryOptions` with a default backend config.

Preferred fix: add a config-aware session constructor and migrate runtime call sites to it.

Suggested API:

```rust
pub fn with_session_config_defaults(
    config: &crate::config::schema::Config,
    todo_state: Arc<tokio::sync::Mutex<crate::task_state::TodoState>>,
    policy: crate::model_profile::types::TaskStatePolicy,
    pool: Option<sqlx::SqlitePool>,
    session_id: Option<String>,
) -> Self {
    Self::with_options(ToolRegistryOptions {
        todo_state: Some(todo_state),
        todo_policy: Some(policy),
        pool,
        session_id,
        lsp_service: None,
        tool_backends: ToolBackendConfig::from_config(config),
    })
}
```

Then either:

- deprecate `with_session_defaults()` and keep it only for tests; or
- change it to call a new `with_session_options()` requiring `ToolBackendConfig`.

Add a doc comment warning that `with_session_defaults()` uses all-native defaults and should not be used in config-aware runtime paths.

Acceptance criteria:

- Runtime session construction uses config-aware backend resolution.
- Tests cover disabled LSP/security in a session registry.
- `with_session_defaults()` is not used in production config-aware paths.

## Phase 3: Decide Disabled Backend Semantics

Current behavior registers a `DisabledTool` under the disabled tool name. This gives an actionable error but still exposes the disabled tool to the model unless later filtered.

Pick one explicit policy:

### Option A: Hidden disabled tools

If a backend is `disabled`, do not register the tool at all.

Pros:

- Reduces model tool surface.
- Matches ordinary meaning of disabled.
- Avoids wasting tool-description tokens.

Cons:

- If a model tries an old/cached tool name, it receives a generic not-found error.

### Option B: Visible disabled placeholder

Keep `DisabledTool` registered but mark it as deferred or hide it from definitions.

Pros:

- Clear actionable error if invoked.
- Useful for diagnostics/testing.

Cons:

- Can pollute provider tool definitions if not filtered.
- Disabled still looks callable.

Recommended implementation: Option A for model-facing tool definitions, with optional internal placeholder only for explicit diagnostics/tests.

Possible approach:

- Add `ToolRegistry::register_disabled_placeholder_for_tests()` only under `#[cfg(test)]`, or
- Register `DisabledTool` but make `definitions()` omit tools whose type/category indicates disabled, or
- Add `Tool::is_exposed()` defaulting to true, overridden by `DisabledTool` to false.

Minimal compatible approach:

```rust
fn expose_in_definitions(&self) -> bool { true }
```

on `Tool`, then skip tools returning false in `ToolRegistry::definitions()` and agent `build_tool_definitions()`.

Acceptance criteria:

- `[tool_backends.lsp].backend = "disabled"` does not expose `lsp` in provider tool definitions unless an explicit debug/test mode opts in.
- Same for `security`.
- Diagnostics still show disabled state.
- Tool registry tests assert disabled tools are not model-visible.

## Phase 4: Reconcile MCP Fallback Semantics

Current risk: `backend_report()` can report `FallbackToNative` for MCP with `fallback_to_native = true`, while actual registration for MCP-unimplemented LSP/security may install a disabled placeholder.

Pick exact behavior for MCP-configured but unimplemented domains.

Recommended first-pass policy:

```text
backend = "mcp", fallback_to_native = true
  -> register native tool
  -> diagnostics status = fallback-native
  -> tool provenance should say native fallback if called

backend = "mcp", fallback_to_native = false
  -> do not expose the tool or expose non-model-visible disabled placeholder
  -> diagnostics status = unavailable/unimplemented
```

Apply this to LSP and security.

Implementation details:

- In `ToolRegistry::with_options()`, for `ToolImplementationBackend::Mcp`, inspect the domain config's `fallback_to_native()`.
- If true, register native wrapper and record/report fallback-native status.
- If false, do not register model-visible tool.
- Add a helper to avoid duplicating this logic for LSP/security.

Acceptance criteria:

- Registration behavior and `backend_report()` agree for all combinations:
  - native;
  - builtin;
  - disabled;
  - mcp + fallback true;
  - mcp + fallback false.
- Tests assert actual `contains("lsp")` / provider definitions for each case.
- No diagnostic row claims fallback-native unless the native wrapper is actually registered.

## Phase 5: Wire `execute_capture()` Into Central Tool Execution

`ToolRegistry::execute_capture()` exists and should be the default tool execution path for native Codegg tools.

Find the central tool execution code. Likely candidates:

```bash
rg "ToolCall" src/agent src/exec src/core src/tui
rg "\.execute\(" src/agent src/exec src/core src/tui
rg "call_tool" src/agent src/exec src/core src/tui
rg "ChatEvent::ToolResult" src
```

Update native Codegg tool execution to call:

```rust
let structured = self.tool_registry.execute_capture(
    &tc.name,
    tc.arguments.clone(),
    Some(ToolExecutionContext {
        backend: ToolBackendKind::Native,
        session_id: Some(self.session_id.clone()),
        cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        permission_mode: None,
        timeout_ms: Some(self.tool_timeout() * 1000),
    }),
).await?;
```

Then pass only `structured.output` back to the model as the tool result.

Preserve existing behavior for MCP tools (`mcp__server__tool`) if they are executed through `McpService::call_tool`; optionally wrap their result in structured provenance at the boundary later, but do not destabilize MCP execution in this pass.

Record provenance internally via tracing. If the event model has a suitable field or extensible metadata, attach provenance there. If not, tracing is sufficient for this pass.

Acceptance criteria:

- Real native tool calls go through `execute_capture()`.
- Model-visible tool output is unchanged.
- Legacy tools without custom `execute_structured()` still work.
- Structured provenance appears in logs/tracing for websearch/webfetch/lsp/security.
- Tests cover one legacy tool and one structured tool through `execute_capture()`.

## Phase 6: Add a Minimal Structured Execution Smoke Test

Add focused tests that do not require network or external MCP servers.

Suggested tests:

```text
tests/tool_structured_execution.rs
```

Cases:

1. `read` or `list` through `execute_capture()` returns legacy provenance.
2. `security` through `execute_capture()` returns `implementation = "eggsec"` or equivalent.
3. Disabled `security` is not model-visible when backend disabled.
4. MCP fallback true registers native security and backend report says fallback-native.
5. MCP fallback false does not expose the tool and backend report says unavailable/unimplemented.

Acceptance criteria:

- Tests run without network.
- Tests do not need actual LSP servers.
- Tests assert both registry state and provider-definition visibility.

## Phase 7: Audit Actual Tool Definition Visibility

The agent has a custom `build_tool_definitions()` path, not just `ToolRegistry::definitions()`. Ensure disabled/non-exposed tools are filtered consistently in both.

Tasks:

- Add one shared helper for converting registered tools into provider definitions.
- Or ensure both `ToolRegistry::definitions()` and agent `build_tool_definitions()` use the same `Tool::expose_in_definitions()` predicate if added.
- Ensure `tool_search` receives the same visible/deferred tool names the model can actually call.

Acceptance criteria:

- Disabled tools do not appear via direct `ToolRegistry::definitions()`.
- Disabled tools do not appear through agent `build_tool_definitions()`.
- `tool_search` does not advertise disabled hidden tools.
- Tests cover both registry definitions and agent/tool-search availability if feasible.

## Phase 8: Final Dependency Ownership Audit

The previous pass moved most LSP-specific dependencies out of root, but root still has some potentially domain-specific dependencies.

Audit:

```bash
rg "notify::|use notify|lsp_types|lsp_server|zip::|xz2|flate2|tar::" src crates
cargo tree -p codegg --edges normal
cargo tree -p egglsp --edges normal
cargo tree -p eggsec --edges normal
cargo tree -p egggit --edges normal
cargo tree -p eggcontext --edges normal
```

Expected outcome:

- Root keeps `flate2`/`tar` only if still used by plugin install or another root feature.
- Root keeps `notify` only if directly used outside `egglsp`; otherwise move it to `egglsp`.
- Root should not directly depend on `lsp-types`, `lsp-server`, `zip`, or `xz2` unless direct imports remain.

Acceptance criteria:

- Each remaining root domain-specific dependency has a nearby comment or obvious direct use.
- No dead root dependency remains after extraction.
- `cargo check --workspace --all-features` still passes.

## Phase 9: Update Architecture Notes and Plan Status

Update docs after code behavior is corrected.

Files to update if behavior changes:

```text
architecture/native_crates.md
architecture/tool.md
architecture/lsp.md
architecture/security.md
architecture/git.md
plans/native_tool_crates_hardening.md
plans/native_tool_runtime_correctness.md
```

Docs should state actual behavior:

- Disabled backend tools are hidden or visible according to the chosen policy.
- MCP fallback behavior is explicit.
- Native tool calls use structured execution internally.
- Provenance is logged/internal, not model-visible.
- `egggit` owns read-only git facts; Codegg owns mutation.

Acceptance criteria:

- Docs no longer describe future behavior as if it already exists.
- Plan status/checklist is updated for completed items.

## Validation Commands

Run before considering this pass complete:

```bash
cargo fmt --all --check
cargo check --workspace --all-features
cargo test --workspace
cargo clippy --workspace --all-features --all-targets
```

If clippy remains noisy, record the specific failures and do not hide compile/test failures behind clippy cleanup.

## Done Criteria

This pass is complete when:

- All real runtime/session registry constructors preserve `[tool_backends]` config.
- `with_session_defaults()` is no longer a production footgun.
- Disabled backend semantics are explicit and tested.
- MCP fallback behavior and diagnostics agree with actual registration.
- Native Codegg tool calls go through `execute_capture()` centrally.
- Structured provenance is available internally without changing model-visible tool output.
- Tool definitions and `tool_search` do not expose hidden disabled tools.
- Root dependencies reflect actual ownership after extraction.
- Tests cover registry visibility, backend fallback, and structured execution.
