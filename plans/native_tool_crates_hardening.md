# Native Tool Crates Hardening and Integration Plan

## Status

> **Superseded by [`native_tool_runtime_correctness.md`](native_tool_runtime_correctness.md).**
>
> The 12 phases of this plan are complete (see commit history). The
> follow-up runtime-correctness pass refined the contracts laid down
> here: `with_session_config_defaults(&Config, ...)` is now the
> production session constructor; `Tool::expose_in_definitions()`
> filters disabled/MCP-stub tools from the model-facing catalog;
> `ToolRegistry::execute_capture(...)` is the central agent-loop
> execution path. The highlights from the original plan are preserved
> below for context.

All 12 phases of this plan are complete (see commit history). The
highlights:

- `ToolRegistry::with_config(&Config)` and
  `ToolRegistryOptions::tool_backends` are the single authoritative
  construction path; `main.rs`, `exec.rs`, and `core/daemon.rs` now
  populate backend config from the loaded config.
- `src/tool/backend_config.rs` is the single explicit
  config-schema → runtime-type conversion site.
- `ToolRegistry::backend_report(mcp_server_names)` reports
  Active / Disabled / ConfiguredButUnavailable / FallbackToNative
  based on actually registered tools plus the resolved config.
- LSP and Security wrappers honour `[tool_backends.lsp|security]`
  by either registering the real tool, a `DisabledTool` stub (for
  `disabled`), or a clear-error stub (for the unimplemented
  `mcp`).
- `Tool::execute_structured()` is the central execution path used
  by the agent loop; `websearch`, `webfetch`, `lsp`, and
  `security` attach real `ToolProvenance` records.
- `egggit::diff_text(root, DiffMode)` plus the `DiffMode` enum is
  the new authoritative read-only diff API; `review.rs` and
  `commit.rs` consume it.
- Root `Cargo.toml` no longer owns `lsp_server` / `lsp-types`
  (those moved to `egglsp`).
- `eggcontext` exposes `TokenEstimate { tokens, tokenizer,
  approximate }` and `TokenizerType::is_approximate()`; Claude and
  Gemini are documented as approximate.
- The agent loop's tool-definition merge in
  `build_tool_definitions` now uses
  `McpService::list_filtered_tools(policy)` with a policy built
  from `[search]` and `[tool_backends.*]`, so managed MCP servers
  are hidden by default.
- `tests/tool_registry.rs` locks down the model-facing tool
  surface.

## Purpose

Tighten the first native-tool-crate extraction pass.

The previous pass created the right structural direction: Codegg now has workspace crates for `egglsp`, `egggit`, `eggcontext`, and `eggsec`, plus an additive backend-aware tool contract in `src/tool/backend.rs`. This follow-up pass should make those boundaries operationally reliable rather than continuing to extract more domains.

The main goal is to make the new crates authoritative substrates where they already exist, ensure runtime behavior matches diagnostics/config, reduce compatibility leakage, and prove the workspace builds/tests cleanly.

## Current State Summary

Codegg now has:

```text
crates/egglsp
crates/egggit
crates/eggcontext
crates/eggsec
```

Root `Cargo.toml` includes them as workspace members and direct path dependencies.

`src/tool/backend.rs` now defines backend/provenance concepts:

```text
ToolBackendKind
ToolExecutionContext
ToolTrust
ToolProvenance
StructuredToolResult
ToolBackendConfig
ExternalToolBackendConfig
ToolImplementationBackend
ToolBackendReport
```

`Tool` has an additive `execute_structured()` method, but most tools still use legacy `execute()`.

`ToolRegistry` now has `ToolRegistryOptions` and `with_options()`, which is a good single registration path. It also supports injected LSP service construction.

`egglsp` and `eggsec` are real extractions, but Codegg still uses compatibility re-export modules under `src/lsp/mod.rs` and `src/security/mod.rs`.

`eggcontext` is currently mostly token estimation.

`egggit` exists with read-only git/worktree facts, but some tools still shell out directly instead of consuming it.

MCP raw-tool exposure now has filtering support via `McpService::list_filtered_tools()` and `McpExposurePolicy`.

## Non-Goals

Do not extract additional crates in this pass.

Do not convert hot-path local filesystem/edit/shell tools into MCP subprocess tools.

Do not rewrite the agent loop or provider layer.

Do not remove compatibility re-exports until all call sites are migrated and tests are green.

Do not implement full repo-map/context-ranking functionality in `eggcontext` yet. Add only enough integration to make the existing token/context extraction sound.

Do not build MCP server binaries for the new crates yet unless a test requires a tiny mock. Native crate integration must be stable first.

## Phase 1: Establish a Green Workspace Baseline

Before architectural cleanup, ensure the new workspace is mechanically sound.

Run:

```bash
cargo fmt --all --check
cargo check --workspace --all-features
cargo test --workspace
cargo clippy --workspace --all-features --all-targets -- -D warnings
```

If `clippy -D warnings` is too noisy for the current repo, record the failures and use normal clippy first:

```bash
cargo clippy --workspace --all-features --all-targets
```

Fix all compile errors before changing behavior.

Pay special attention to:

- renamed LSP files now under `crates/egglsp`;
- `wasmtime` / `wasmtime-wasi` version mismatches;
- root crate dependencies that are now duplicated in extracted crates;
- `crate::lsp::*` compatibility imports;
- `crate::security::*` compatibility imports;
- `ToolBackendConfigSchema` references from `tool/backend.rs`;
- root `Cargo.lock` churn.

Acceptance criteria:

- `cargo check --workspace --all-features` passes.
- `cargo test --workspace` passes.
- Any remaining clippy failures are documented in this plan or a follow-up issue with exact commands and errors.

## Phase 2: Add Explicit Config-to-Runtime Backend Conversion

There are currently two backend config representations:

Config-time schema in `src/config/schema.rs`:

```rust
ToolBackendConfigSchema
ExternalToolBackendConfigSchema
ToolImplementationBackendSchema
```

Runtime/tool-side schema in `src/tool/backend.rs`:

```rust
ToolBackendConfig
ExternalToolBackendConfig
ToolImplementationBackend
```

That split is acceptable, but it needs one explicit conversion path. Add conversion helpers, preferably in `src/tool/backend.rs` or a small `src/tool/backend_config.rs` module.

Suggested API:

```rust
impl From<&crate::config::schema::ToolBackendConfigSchema> for ToolBackendConfig {
    fn from(schema: &crate::config::schema::ToolBackendConfigSchema) -> Self { ... }
}

impl From<&crate::config::schema::ExternalToolBackendConfigSchema> for ExternalToolBackendConfig {
    fn from(schema: &crate::config::schema::ExternalToolBackendConfigSchema) -> Self { ... }
}

impl From<crate::config::schema::ToolImplementationBackendSchema> for ToolImplementationBackend {
    fn from(schema: crate::config::schema::ToolImplementationBackendSchema) -> Self { ... }
}
```

Also add a helper:

```rust
impl ToolBackendConfig {
    pub fn from_config(config: &crate::config::schema::Config) -> Self {
        config
            .tool_backends
            .as_ref()
            .map(Self::from)
            .unwrap_or_else(Self::all_native)
    }
}
```

Then update real startup/registry construction paths to pass the converted config into `ToolRegistryOptions`.

Search for all callers of:

```rust
ToolRegistry::with_defaults()
ToolRegistry::with_session_defaults(...)
ToolRegistryOptions::default()
```

Make sure session construction does not silently lose configured tool backends.

Acceptance criteria:

- `ToolBackendConfigSchema` is not consumed ad hoc from multiple places.
- `ToolRegistryOptions.tool_backends` is populated from loaded config in real runtime paths.
- Defaults remain all-native for `lsp`, `security`, and `context` unless explicitly configured otherwise.
- Unit tests cover conversion for `native`, `mcp`, `builtin`, and `disabled`.

## Phase 3: Make Diagnostics Reflect Actual Runtime Behavior

`build_report()` currently reports backend state partly from config and partly from hardcoded assumptions. Tighten it so diagnostics cannot claim a backend is disabled/MCP while the registry still exposes a native wrapper that ignores that config.

Add a runtime status source. Options:

1. Minimal option: `ToolRegistry` exposes a `backend_report()` method that uses the actual registered tools plus config.
2. Better option: introduce a `ToolBackendStatusProvider` trait for wrappers that can report current backend state.

Minimal first-pass API:

```rust
impl ToolRegistry {
    pub fn backend_report(
        &self,
        search: &SearchConfig,
        tool_backends: &ToolBackendConfig,
        mcp_server_names: Option<&[String]>,
    ) -> ToolBackendReport { ... }
}
```

The report should distinguish:

```text
registered + native
registered + mcp
registered + disabled wrapper
not registered because disabled
configured mcp but server unavailable
configured mcp with fallback_to_native
```

Do not report a backend as active solely because config says so if the wrapper still ignores it.

Acceptance criteria:

- `/tool-backends` or equivalent output is built from actual registered state where possible.
- If `[tool_backends.lsp].backend = "disabled"`, diagnostics and tool registration agree.
- If `[tool_backends.security].backend = "disabled"`, diagnostics and tool registration agree.
- If a Codegg-managed MCP backend is hidden, diagnostics still show it for status but it is not exposed to the model by default.

## Phase 4: Enforce Backend Config for LSP and Security Wrappers

The backend config should affect behavior for at least two non-search domains in this pass: `lsp` and `security`.

### LSP behavior

For `[tool_backends.lsp]`:

```toml
backend = "native"     # default
backend = "disabled"   # do not expose lsp, or expose clear disabled error
backend = "mcp"        # optional placeholder; error clearly unless implemented
```

Recommended first-pass behavior:

- `native`: register normal `LspTool` backed by `egglsp`.
- `disabled`: do not register `lsp`, or register `DisabledTool` with a clear error. Prefer not registering if the tool catalog handles it cleanly.
- `mcp`: do not implement actual MCP delegation yet. Register a wrapper only if it returns a clear error: `lsp MCP backend is configured but not implemented; set backend = "native" or "disabled"`.
- `builtin`: treat same as `native` for now or reject as unsupported. Document choice.

### Security behavior

For `[tool_backends.security]`:

- `native`: register `SecurityTool` backed by `eggsec`.
- `disabled`: do not register `security`, or register clear disabled error.
- `mcp`: placeholder clear error unless an actual `eggsec` MCP adapter exists.
- `builtin`: same as `native` only if documented.

A small generic disabled wrapper can be useful:

```rust
pub struct DisabledTool {
    name: &'static str,
    description: &'static str,
    reason: String,
}
```

But do not register disabled wrappers if that pollutes the model tool list. Prefer unregistering unless the user experience needs a visible error.

Acceptance criteria:

- LSP and security registration depends on resolved backend config.
- Disabled domains do not silently register active tools.
- MCP-configured-but-unimplemented domains produce actionable errors or are rejected at config validation.
- Tests cover native and disabled registration for both domains.

## Phase 5: Wire Structured Tool Results Into Actual Execution

`execute_structured()` exists, but it needs at least minimal use in the agent/tool execution path.

Find the central location where tool calls are executed. Update it to prefer:

```rust
tool.execute_structured(input, Some(ctx)).await
```

Then convert to the legacy string output for the model:

```rust
let structured = tool.execute_structured(input, Some(ctx)).await?;
let output_for_model = structured.output.clone();
```

Record provenance internally:

- tracing span fields;
- session event metadata if available;
- future `/tool-backends` diagnostics if appropriate.

Do not expose provenance JSON to the model by default.

Add custom structured implementations for at least:

- `websearch`;
- `webfetch`;
- `lsp`;
- `security`.

For `websearch` / `webfetch`, provenance should show whether backend was `eggsearch` MCP, built-in fallback, disabled, or unavailable.

For `lsp`, provenance should show `Native` + `egglsp`.

For `security`, provenance should show `Native` + `eggsec`.

Acceptance criteria:

- Central tool execution calls `execute_structured()`.
- Model-visible output remains unchanged.
- At least four tools emit provenance.
- Tests verify that legacy-only tools still work through the structured path.

## Phase 6: Make `egggit` the Authoritative Read-Only Git Substrate

`egggit` exists, but some Codegg tools still shell out to git directly for read-only facts. Consolidate those paths.

Refactor targets:

1. `src/tool/review.rs`

Replace direct `tokio::process::Command::new("git")` diff logic with `egggit` APIs.

Current behavior accepts `staged: bool`. Add or reuse an `egggit` API that can return staged or unstaged/full diff:

```rust
pub async fn diff_text(root: &Path, mode: DiffMode) -> Result<String, EgggitError>;

pub enum DiffMode {
    Head,
    Staged,
    Base(String),
}
```

Or adapt existing `file_diff` / `diff_summary` if it already supports this.

2. `src/tool/commit.rs`

Do not move mutation into `egggit`, but use `egggit` for deterministic pre-commit facts: changed files, diff summary, staged diff validation.

3. `src/worktree/mod.rs`

Use `egggit::worktree` APIs where possible. Keep mutating worktree create/remove in Codegg permission flow unless `egggit` explicitly exposes a permission-neutral planner API.

4. Architecture docs

Update `architecture/git.md`, `architecture/worktree.md`, and `architecture/tool.md` to state that `egggit` owns read-only facts and Codegg owns mutation/permission.

Acceptance criteria:

- `review` no longer shells out directly for diff text unless through `egggit`.
- Commit generation consumes at least one `egggit` fact API.
- Worktree listing uses `egggit` if applicable.
- Mutating git operations remain in Codegg and remain permission-gated.
- Tests cover staged and unstaged review diff acquisition.

## Phase 7: Tighten `egglsp` Boundary and Dependency Ownership

`src/lsp/mod.rs` currently re-exports `egglsp` modules to preserve old call sites. Keep that compatibility for now, but reduce root dependency leakage and document the boundary.

Tasks:

1. Audit root `Cargo.toml` for dependencies that now belong only to `egglsp`:

```text
lsp-types
lsp-server
zip
flate2
tar
xz2
notify
```

Move dependencies into `crates/egglsp/Cargo.toml` if the root crate no longer imports them directly.

2. Search direct imports:

```bash
rg "lsp_types|lsp_server|zip::|flate2|tar::|xz2|notify::" src crates
```

3. Prefer direct `egglsp::...` imports at new/edited Codegg boundaries. Do not churn all call sites unnecessarily.

4. Keep `src/lsp/mod.rs` as a documented compatibility shim.

Acceptance criteria:

- Root crate no longer owns LSP-only dependencies unless directly used.
- `egglsp` builds independently enough for `cargo test -p egglsp`.
- Codegg LSP tool continues to expose the same model-facing schema.
- Compatibility shim is documented as temporary or boundary-only.

## Phase 8: Tighten `eggsec` Boundary and Dependency Ownership

`eggsec` should own deterministic scan/classification logic. Codegg should own policy, sandboxing, SSRF, sensitive paths, and enforcement.

Tasks:

1. Ensure these remain in Codegg:

```text
src/security/policy.rs
src/security/sandbox.rs
src/security/service.rs
src/security/ssrf.rs
sensitive path matching
permission escalation
```

2. Ensure these live in `eggsec`:

```text
command classification
secret/text scanning
dependency file detection
profile runner if deterministic and policy-neutral
finding types
```

3. Add conversion from `eggsec::EggsecError` to `ToolError` or `AppError` in one place.

4. Make `SecurityTool` imports prefer `eggsec::...` directly or keep the re-export shim, but do not mix both styles across new code.

5. Add tests in both places:

- `eggsec` unit tests for findings;
- Codegg wrapper tests for tool schema and output shape.

Acceptance criteria:

- `cargo test -p eggsec` passes.
- Codegg security tool remains read-only.
- No Codegg policy/enforcement logic moved into `eggsec`.
- `SecurityTool` clearly describes deterministic scanning, not LLM security review.

## Phase 9: Stabilize `eggcontext` Without Expanding Scope Too Far

`eggcontext` currently provides token estimation. Make that narrow API reliable before implementing repo maps or ranking.

Tasks:

1. Fix model tokenizer mapping.

Current logic treats some modern model families heuristically. Add tests for the model names Codegg actually uses in profiles/catalog examples.

2. Decide whether `estimate_tokens_sync` is approximate or exact. Document it explicitly.

Suggested naming if approximate:

```rust
estimate_tokens_approx
```

Or keep existing name but document approximation for Claude/Gemini multipliers.

3. Add a `TokenEstimate` type with provenance:

```rust
pub struct TokenEstimate {
    pub tokens: usize,
    pub tokenizer: TokenizerType,
    pub approximate: bool,
}
```

Keep the old `estimate_tokens_sync()` as a compatibility wrapper.

4. Update `ContextTracker` to optionally use the richer estimate internally if useful.

Acceptance criteria:

- `cargo test -p eggcontext` passes.
- Token estimation behavior is documented as approximate where appropriate.
- Existing Codegg compaction tests still pass.
- No repo-map/ranking implementation is added in this pass.

## Phase 10: Raw MCP Exposure Policy Integration

`McpService::list_filtered_tools()` exists. Ensure actual provider/tool-definition construction uses it where relevant.

Search for:

```rust
mcp_service.list_tools()
list_filtered_tools
mcp__
expose_raw_mcp_tools
```

Tasks:

1. Identify the central place where MCP tool definitions are merged into provider tool definitions.

2. Replace raw `list_tools()` calls with `list_filtered_tools(policy)` where user/config policy is available.

3. Define policy rules:

- Codegg-managed backend servers with native wrappers are hidden by default.
- User-configured third-party MCP servers remain exposed by default unless disabled.
- `search.expose_raw_mcp_tools = true` exposes eggsearch raw tools.
- Future `[tool_backends.*].expose_raw_mcp_tools = true` exposes raw managed backend tools for that domain.

4. Keep `/mcps` or diagnostics able to show hidden managed servers.

Acceptance criteria:

- Raw `mcp__eggsearch__*` remains hidden by default when native `websearch`/`webfetch` are active.
- Third-party MCP tools still appear by default.
- Tests cover hidden managed server and visible third-party server behavior.

## Phase 11: Tool Schema and Registry Snapshot Tests

The model-facing tool surface must stay stable through these refactors.

Add snapshot-style tests that assert at least:

- default registry includes expected native tool names;
- disabled LSP/security backend removes or disables those tools as intended;
- `websearch` and `webfetch` remain native names;
- raw managed MCP tools are hidden by default;
- `tool_search` catalog still sees the intended discoverable tools;
- `ToolCategory` values for important tools remain correct.

Suggested test module:

```text
tests/tool_registry.rs
```

Acceptance criteria:

- Failing schema changes are caught in tests.
- Tests are deterministic and do not require network access or external MCP binaries.
- Tool names and categories are asserted separately from long descriptions to reduce brittleness.

## Phase 12: Documentation Cleanup

Update docs to reflect the actual post-hardening state, not the intended future state.

Files likely needing updates:

```text
architecture/native_crates.md
architecture/tool.md
architecture/lsp.md
architecture/security.md
architecture/git.md
architecture/worktree.md
plans/native_tool_crates.md
plans/native_tool_crates_hardening.md
```

Docs should clearly state:

- Codegg is library-first and MCP-second.
- `egglsp` owns LSP implementation; `src/lsp` is a compatibility/boundary shim.
- `eggsec` owns deterministic scanning; Codegg owns policy/enforcement/sandboxing.
- `egggit` owns read-only git facts; Codegg owns mutating git operations and permission flow.
- `eggcontext` currently owns token estimation only; repo maps/ranking are future work.
- Raw MCP tools for Codegg-managed backend servers are hidden by default.

Acceptance criteria:

- Architecture docs match code behavior.
- No doc claims MCP backends are implemented for `lsp`/`security` unless they actually are.
- Plan status is updated if phases are completed.

## Recommended Implementation Order

Do this in order:

1. Green workspace baseline.
2. Config-to-runtime backend conversion.
3. Backend-aware registry behavior for LSP/security.
4. Diagnostics aligned with actual registration.
5. Structured execution path wired into central tool execution.
6. `egggit` consumed by review/commit/worktree read-only paths.
7. Dependency ownership cleanup for `egglsp` and `eggsec`.
8. `eggcontext` token-estimate stabilization.
9. MCP raw exposure policy applied at the actual provider/tool-definition merge point.
10. Tool registry/schema tests.
11. Documentation cleanup.

Do not start by adding MCP adapter binaries. The native crate path needs to become boring and reliable first.

## Done Criteria

This hardening pass is complete when:

- The workspace checks and tests pass.
- `ToolRegistryOptions.tool_backends` is populated from loaded config in real startup/session paths.
- LSP/security backend config changes actual tool registration or behavior.
- Diagnostics match actual runtime behavior.
- Central tool execution uses `execute_structured()` and records provenance without changing model-visible output.
- `egggit` is used by at least review and one other git/worktree consumer.
- Root dependencies are reduced where crate ownership has moved.
- `eggcontext` token estimation is documented and tested as approximate/exact as appropriate.
- Raw Codegg-managed MCP tools are hidden by default, while user third-party MCP tools remain visible.
- Tool registry/schema tests protect the model-facing contract.
