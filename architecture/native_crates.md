# Native Tool Crates

Codegg follows a **library-first, MCP-second** tool architecture. Durable
tool domains live in workspace crates under `crates/` and are consumed
directly in-process. The same crates can later expose optional MCP
adapter binaries without changing the model-facing tool names.

This document describes the runtime contract (Phase 1 of the plan), the
backend-selection policy (Phase 3 / Phase 9), and the per-crate
boundaries. See `plans/native_tool_crates.md` for the full plan and
`architecture/tool.md` for the tool registry side of the contract.

## Workspace layout

```
crates/
  egglsp/       Language Server Protocol client/service/operations
  egggit/       Read-only git facts: status, diff, changed files, worktrees
  eggsec/       Deterministic security scanning: secrets, commands, deps, profiles
  eggcontext/   Token counting + context utilities (tiktoken)
```

The top-level `codegg` package is a workspace member and depends on
each of these crates.

## Codegg ↔ crate boundary

| Side | Direction | Notes |
|------|-----------|-------|
| Codegg config types | codegg → crate | Codegg converts `crate::config::schema::*` into crate-local config types via `From` impls in `src/tool/backend_config.rs` |
| Crate config types | crate → codegg | Crates never import codegg config types |
| `Tool` trait | codegg | Native wrappers in `src/tool/*.rs` implement the trait and call into the crates |
| Permission gating | codegg | PermissionChecker is authoritative; crates may classify operations but cannot weaken policy |
| Output provenance | crate → codegg | Crates report `ToolTrust` so logs/UI can frame outputs consistently |
| Tests | both | Each crate has self-contained tests; codegg-side wrapper tests cover schema stability and backend wiring |

## Runtime contract (`src/tool/backend.rs`)

A small in-process contract for backend-aware tool execution. The
`Tool` trait gains one optional method (`execute_structured`) with a
default implementation, so existing tools keep working without
changes. New wrappers and crate-driven backends opt into structured
execution.

```rust
pub enum ToolBackendKind { Native, Mcp, Shell, BuiltinLegacy }

pub struct ToolExecutionContext {
    pub backend: ToolBackendKind,
    pub session_id: Option<String>,
    pub cwd: std::path::PathBuf,
    pub permission_mode: Option<String>,
    pub timeout_ms: Option<u64>,
}

pub struct ToolProvenance {
    pub backend: String,
    pub implementation: String,
    pub version: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub truncated: bool,
    pub trust: ToolTrust,
}

pub enum ToolTrust { LocalTrusted, LocalUntrusted, ExternalUntrusted, MutatingSideEffect }

pub struct StructuredToolResult {
    pub output: String,
    pub success: bool,
    pub provenance: Option<ToolProvenance>,
}
```

`StructuredToolResult::legacy(name, output)` is the bridge for tools
that have not yet adopted structured execution; `into_legacy_output()`
extracts the string for model-facing calls.

The central agent-loop execution path in `src/agent/loop.rs` goes
through `ToolRegistry::execute_capture(name, input, ctx)` for every
native Codegg tool call (replacing direct `t.execute_structured()`
calls at the call site). The model-facing string output
(`structured.output`) is unchanged; the `ToolProvenance` returned to
the caller is recorded internally via `tracing::debug!` plus the
elapsed time and trust metadata. MCP tools (`mcp__server__tool`)
continue to dispatch through `McpService::call_tool` and are not
funnelled through `execute_capture` in this pass.

## Backend selection config

Per-domain backend configuration is parsed from TOML/JSON via
`config::schema::ToolBackendConfigSchema`:

```toml
[tool_backends.lsp]
backend = "native"          # native | mcp | builtin | disabled
fallback_to_native = true
expose_raw_mcp_tools = false
server_name = "egglsp"
timeout_ms = 30000
```

The runtime-facing `tool::backend::ToolBackendConfig` (and its
helpers `ToolBackendKind::parse`, `parse_implementation`) is built
from the schema on startup and is the single source of truth for
which backend handles which model-facing tool. The conversion
lives in `src/tool/backend_config.rs`:

```rust
impl From<&ToolBackendConfigSchema> for ToolBackendConfig { ... }
impl ToolBackendConfig::from_config(&Config) -> Self { ... }
```

When the `[tool_backends]` section is absent, the runtime falls back
to `ToolBackendConfig::all_native()` so domains without explicit
configuration stay authoritative native.

### ToolRegistry honours the config

`ToolRegistry::with_options(ToolRegistryOptions { tool_backends, .. })`
is the single authoritative construction path. The resolved
`tool_backends` are stashed on the registry (`registry.tool_backends()`)
and consulted by:

- `ToolRegistry::backend_report(mcp_server_names)` for `/tool-backends`
  diagnostics
- `with_options` to decide between the real `LspTool` / `SecurityTool`,
  a `DisabledTool` stub (hidden from the model), and the
  fallback-native wrapper
- `agent/loop.rs::build_tool_definitions` to build the MCP exposure
  policy used by `McpService::list_filtered_tools`

### Session construction: `with_session_config_defaults` vs `with_session_defaults`

There are two session constructors. Production code paths (the agent
loop, the daemon) must use the config-aware one:

- `ToolRegistry::with_session_config_defaults(&Config, todo_state,
  policy, pool, session_id)` — resolves
  `ToolBackendConfig::from_config(&Config)` and threads it through
  `with_options`. **This is the constructor real session code uses.**
- `ToolRegistry::with_session_defaults(todo_state, policy, pool,
  session_id)` — kept for tests and non-config-aware callers; it
  builds `ToolBackendConfig::default()` so it **silently drops any
  loaded `[tool_backends]` config**. The doc comment explicitly warns
  against using it in production paths that have access to a
  `Config`.

The split was introduced so a config-aware path can never accidentally
end up with the all-native default for LSP/security.

### MCP fallback semantics

`with_options` consults `ToolBackendConfig` for each configurable
domain (LSP, security) and applies this matrix, which `backend_report`
mirrors exactly:

| `backend` setting | `fallback_to_native` | Registered tool | `backend_report` status |
|-------------------|----------------------|-----------------|-------------------------|
| `native` / `builtin` | (any) | real `LspTool` / `SecurityTool` wrapper | `ready` |
| `mcp` | `true` (default) | real native wrapper (live path is the native crate, not the MCP server) | `fallback-native` |
| `mcp` | `false` | hidden `DisabledTool` stub (model never sees it) | `unavailable` (`ConfiguredButUnavailable`) regardless of whether the MCP server is connected |
| `disabled` | (any) | hidden `DisabledTool` stub (model never sees it) | `disabled` |

`DisabledTool` overrides `Tool::expose_in_definitions()` to `false`,
so it is registered in the registry (callable for diagnostics and
tests) but filtered out of the model-facing tool definitions and
`tool_search` results.

### Model-facing definition visibility

`Tool::expose_in_definitions()` (default `true`) is the model-facing
predicate. `ToolRegistry::definitions()` and
`AgentLoop::build_tool_definitions()` both filter through it, so
disabled/MCP-stub tools are hidden from the model but remain callable
by name for diagnostics and `/tool-backends` reports.

## Raw MCP exposure policy

Codegg-owned backend MCPs (the default for `eggsearch`, and any
future `egglsp`/`eggsec` MCP adapters) are hidden by default. The
`McpService::list_filtered_tools(policy)` API takes an
`McpExposurePolicy { show_raw, hidden_servers }` and returns either
all `mcp__*` tools, or only the non-managed subset. The agent loop
now constructs this policy in `build_tool_definitions` from the
resolved `SearchConfig` and per-domain `[tool_backends.*]` config so:

- `websearch`/`webfetch` raw `mcp__eggsearch__*` are hidden unless
  `[search].expose_raw_mcp_tools = true`
- A future `egglsp` MCP adapter would be hidden by default, with
  `[tool_backends.lsp].expose_raw_mcp_tools = true` opting in
- User-configured third-party MCP servers remain visible

## Diagnostics

`/tool-backends` (aliases `/tools`, `/backends`) renders a textual
report showing:

```
Tool         Backend   Implementation    Status       Raw MCP exposed
websearch    MCP       eggsearch          ready        no
webfetch     MCP       eggsearch          ready        no
lsp          Native    egglsp             ready        n/a
security     Native    eggsec             ready        n/a
git          Native    codegg/egggit      ready        n/a
```

There are now two report sources:

1. `tool::backend::build_report(&SearchConfig, Option<&ToolBackendConfigSchema>, Option<&[String]>)`
   — synchronous, config-only. Used by the TUI toast.
2. `ToolRegistry::backend_report(Option<&[String]>)` — runtime-aware
   and returns `Vec<RegistryBackendStatus>` (Active | Disabled |
   ConfiguredButUnavailable | FallbackToNative). Used by tests and
   any future diagnostic that has access to the live registry.

## Per-crate public APIs

### `eggsec`

- `command::classify_bash_command`, `classify_git_subcommand`, `classify_tool_call`
- `command::CommandClassification`, `CommandRisk`
- `dependency::detect_dependency_file`, `recommended_audit_commands`, `DependencyEcosystem`
- `finding::SecurityFinding`, `SecurityReport`, `Severity`, `Confidence`, `SecurityCategory`
- `profile::ProfileRunner`, `SecurityProfile`, `ProfileConfig`
- `scanner::inspect_file`, `inspect_text`
- `EggsecError { Io, FileTooLarge, Join }` — bridged to `ToolError`
  in `src/error.rs`.

### `eggcontext`

- `TokenizerType::{Cl100kBase, Claude, Gemini, O200kBase}` with `for_model`, `multiplier`, `is_approximate`
- `TokenEstimate { tokens, tokenizer, approximate }`
- `estimate_with_provenance(text, model) -> TokenEstimate`
- `estimate_tokens_sync(text, model) -> usize` (compatibility wrapper; approximate for Claude/Gemini)
- `estimate_tokens(text) -> usize`
- `EggcontextError`

### `egggit`

- `repo_status(root) -> RepoStatus`
- `diff_summary(root, base) -> DiffSummary`
- `diff_text(root, mode) -> String` with `DiffMode::{Head, Staged, Base(&'static str)}`
- `changed_files(root, base) -> Vec<ChangedFile>`
- `file_diff(root, path, base) -> FileDiff`
- `validate_patch(root, patch) -> PatchValidation`
- `list_worktrees(git_root) -> Vec<WorktreeInfo>` (async)
- `find_git_root`, `is_git_file`, `is_git_worktree`
- `EgggitError { Io, Git, NotARepository, InvalidBaseRef, Join }`

### `egglsp`

- `LspConfig { Disabled, Rules(HashMap<String, LspRule>) }`
- `LspRule { Disabled, Active { command, extensions, env, initialization } }`
- `LspService::new(config)`, `open_file`, `update_file`, `close_file`, `save_file`, `shutdown_all`
- `LspOperations::go_to_definition`, `find_references`, `hover`, `document_symbols`, `code_actions`, `code_lens`
- `DiagnosticsCollector`
- `LspError { ServerNotFound, DownloadFailed, LaunchFailed, NotInitialized, RequestFailed, RequestTimeout, UnsupportedLanguage, Io, Json }`

## Codegg-side bridge files

When a Codegg config type needs to flow into a crate, conversion
happens in a dedicated `*_bridge` style site near the top of the
relevant module (no glob-re-exports across the boundary). For example:

- `src/lsp/mod.rs` is a thin **compatibility shim** that re-exports
  the `egglsp` module tree, converts
  `crate::config::schema::LspConfig` → `egglsp::LspConfig`, and
  bridges `egglsp::LspError` → `crate::error::LspError`. New code
  should prefer direct `egglsp::...` imports.
- `src/security/mod.rs` keeps policy, sandboxing, SSRF, and
  sensitive-path matching in Codegg; the `eggsec` re-exports under
  the `command`, `dependency`, `finding`, `profile`, and `scanner`
  submodules are kept for backward compatibility but new code
  should import directly from `eggsec::...`.
- `src/worktree/mod.rs` keeps the mutating worktree operations
  (`create_worktree`, `remove_worktree`) and re-exports
  `list_worktrees` from `egggit` after wrapping the result in the
  legacy `Worktree` shape used by callers.

## Test strategy

- Each crate has self-contained unit tests in `crates/<name>/src/*.rs`.
- Codegg wrapper tests snapshot the model-facing JSON schemas
  (`security::parameters_schema_snapshot`,
  `lsp::lsp_parameters_schema_snapshot`).
- `tests/tool_registry.rs` (added in the hardening pass) locks down
  the model-facing tool surface: required tool names, tool
  categories, and disabled/missing tool behaviour across backend
  configs.
- `tests/tool_structured_execution.rs` (added in the runtime
  correctness pass) locks down the structured execution and
  definition-visibility contracts: legacy tools return
  `ToolProvenance::legacy`; `security` returns
  `implementation = "eggsec"`; disabled `security` is filtered from
  definitions; `mcp + fallback_to_native = true` registers the
  native wrapper and reports `fallback-native`;
  `mcp + fallback_to_native = false` exposes nothing and reports
  `unavailable`.
- Failure paths (missing backend binary, disabled backend,
  malformed input) are tested at the dispatch layer
  (`tool::backend::report_tests`,
  `tool::backend_report_tests::*`).
- Integration tests that exercise real subprocesses (e.g.
  `tests/worktree.rs`) call the now-`async` APIs via
  `#[tokio::test]`.
