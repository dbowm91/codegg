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
| Codegg config types | codegg → crate | Codegg converts `crate::config::schema::*` into crate-local config types via `From` impls |
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
which backend handles which model-facing tool.

## Raw MCP exposure policy

Codegg-owned backend MCPs (the default for `eggsearch`, and any
future `egglsp`/`eggsec` MCP adapters) are hidden by default. The
`McpService::list_filtered_tools(policy)` API takes an
`McpExposurePolicy { show_raw, hidden_servers }` and returns either
all `mcp__*` tools, or only the non-managed subset. The
`SearchConfig::expose_raw_mcp_tools` flag toggles this for
`websearch`/`webfetch`; future per-server policies live in the same
`McpExposurePolicy` shape.

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

`tool::backend::build_report()` constructs the report synchronously
from the resolved backend config plus any pre-installed context (e.g.
eggsearch availability from `search_backend::state`).

## Per-crate public APIs

### `eggsec`

- `command::classify_bash_command`, `classify_git_subcommand`, `classify_tool_call`
- `command::CommandClassification`, `CommandRisk`
- `dependency::detect_dependency_file`, `recommended_audit_commands`, `DependencyEcosystem`
- `finding::SecurityFinding`, `SecurityReport`, `Severity`, `Confidence`, `SecurityCategory`
- `profile::ProfileRunner`, `SecurityProfile`, `ProfileConfig`
- `scanner::inspect_file`, `inspect_text`
- `EggsecError { Io, FileTooLarge, Join }`

### `eggcontext`

- `TokenizerType::{Cl100kBase, Claude, Gemini, O200kBase}` with `for_model`, `multiplier`
- `estimate_tokens_sync(text, model) -> usize`
- `estimate_tokens(text) -> usize`
- `EggcontextError`

### `egggit`

- `repo_status(root) -> RepoStatus`
- `diff_summary(root, base) -> DiffSummary`
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

- `src/lsp/mod.rs` is a thin wrapper that converts `crate::config::schema::LspConfig` → `egglsp::LspConfig` and bridges `egglsp::LspError` → `crate::error::LspError`.
- `src/worktree/mod.rs` keeps the mutating worktree operations (`create_worktree`, `remove_worktree`) and re-exports `list_worktrees` from `egggit` after wrapping the result in the legacy `Worktree` shape used by callers.
- `src/security/mod.rs` re-exports `eggsec::{command, dependency, finding, profile, scanner}` so internal call sites that still use `crate::security::finding::Severity` etc. keep working.

## Test strategy

- Each crate has self-contained unit tests in `crates/<name>/src/*.rs`.
- Codegg wrapper tests snapshot the model-facing JSON schemas (`security::parameters_schema_snapshot`, `lsp::lsp_parameters_schema_snapshot`).
- Failure paths (missing backend binary, disabled backend, malformed input) are tested at the dispatch layer (`tool::backend::report_tests`).
- Integration tests that exercise real subprocesses (e.g. `tests/worktree.rs`) call the now-`async` APIs via `#[tokio::test]`.
