# Native Tool Crates

Codegg follows a **library-first, MCP-second** tool architecture. Durable
tool domains live in workspace crates under `crates/` and are consumed
directly in-process. The same crates can later expose optional MCP
adapter binaries without changing the model-facing tool names.

External search is the explicit exception: eggsearch is the shared
MCP-owned service for provider routing, credentials, and retrieval.
Codegg's wrappers consume that boundary rather than add provider clients
under `src/search/*`.

This document describes the runtime contract, backend-selection policy,
and per-crate boundaries. See `architecture/tool.md` for the tool
registry side of the contract.

## Workspace Layout

```
crates/
  codegg-core/       Core runtime, session, storage, domain state, jobs,
                     workspace, tool programs (27 modules)
  codegg-config/     Configuration schema, paths, loading, validation,
                     file watching
  codegg-protocol/   Core protocol types: CoreRequest, CoreResponse,
                     CoreEvent, TuiMessage, UiNode, UiEffect
  codegg-providers/  LLM provider implementations, auth types,
                     CircuitBreaker
  codegg-git/        Typed Git operation model, argv parser, risk
                     classification (pure data, no subprocess)
  egglsp/            Language Server Protocol client/service/operations
  egggit/            Read-only git facts: status (v2 rich), diff, log,
                     blame, refs, conflicts, operation state, worktrees
  eggsentry/         Deterministic security scanning: secrets, commands,
                     deps, profiles
  eggcontext/        Token counting + context utilities (tiktoken)
```

Non-member binary crate:
```
  egglsp-test-server/  Fake LSP server for integration tests
                       (NOT a workspace member; binary in root
                       Cargo.toml behind lsp-test-support feature)
```

Workspace members (10 total): root `codegg` + 9 crates under `crates/`.

## Codegg ↔ Crate Boundary

| Side | Direction | Notes |
|------|-----------|-------|
| Codegg config types | codegg → crate | Root converts `config::schema::*` into crate-local types via `From` impls |
| Crate config types | crate → codegg | Crates never import codegg config types |
| `Tool` trait | codegg | Native wrappers in `src/tool/*.rs` implement the trait and call into crates |
| Permission gating | codegg | PermissionChecker is authoritative; crates classify but cannot weaken policy |
| Output provenance | crate → codegg | Crates report `ToolTrust` so logs/UI frame outputs consistently |
| Tests | both | Each crate has self-contained tests; wrapper tests cover schema stability |

## Dependency Graph (verified from Cargo.toml)

```
codegg (root)
  ├── codegg-core
  │     ├── codegg-config
  │     ├── codegg-git
  │     ├── codegg-protocol
  │     ├── codegg-providers
  │     │     └── codegg-config
  │     ├── egggit
  │     ├── egglsp
  │     └── eggsentry
  ├── codegg-config
  ├── codegg-protocol
  ├── codegg-providers
  ├── codegg-git
  ├── egggit
  ├── egglsp
  ├── eggsentry
  └── eggcontext
```

Note: `codegg-core` depends on `codegg-git` (typed Git model), but
`eggcontext` is NOT a dependency of `codegg-core` (consumed only by
root).

## Runtime Contract (`src/tool/backend.rs`)

A small in-process contract for backend-aware tool execution:

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

`StructuredToolResult::legacy(name, output)` bridges tools that have not
yet adopted structured execution. The agent loop goes through
`ToolRegistry::execute_capture(name, input, ctx)` for every native tool
call.

## Backend Selection Config

Per-domain backend configuration parsed from TOML/JSON:

```toml
[tool_backends.lsp]
backend = "native"          # native | mcp | builtin | disabled
fallback_to_native = true
expose_raw_mcp_tools = false
server_name = "egglsp"
timeout_ms = 30000
```

Runtime conversion: `ToolBackendConfig::from_config(&Config)` in
`src/tool/backend_config.rs`. When `[tool_backends]` is absent, falls
back to `ToolBackendConfig::all_native()`.

### MCP Fallback Matrix

| `backend` | `fallback_to_native` | Registered tool | Status |
|-----------|----------------------|-----------------|--------|
| `native` / `builtin` | (any) | real wrapper | `ready` |
| `mcp` | `true` | native wrapper (fallback) | `fallback-native` |
| `mcp` | `false` | hidden `DisabledTool` | `unavailable` |
| `disabled` | (any) | hidden `DisabledTool` | `disabled` |

## Per-Crate Public APIs

### `codegg-core`

27 modules covering core runtime, session, storage, bus, error, goal,
identity, jobs, memory, migration, model_profile, project_catalog,
project_discovery, project_discovery_service, project_storage,
projection_replay, protocol_conversions, provider_connections,
repository_lineage, resilience, run_store, session, snapshot, storage,
task_state, tool_program, workspace, workspace_services, worktree.

Key types: `AppError`, `GlobalEventBus`, `PermissionRegistry`,
`QuestionRegistry`, `TodoState`, `ResolvedModelProfile`,
`ResolvedModelAdapter`, `TaskStatePolicy`, `WorkspaceId`,
`ExecutionContext`, `WorkspaceRegistry`, `JobId`, `AttemptId`,
`RunStore`.

See `architecture/codegg_core.md` for the full module map.

### `codegg-config`

- `Config` — top-level configuration struct
- `schema::*` — all config schema types (model profiles, tool backends,
  search, LSP, security, etc.)
- `ConfigError`, `AppError`
- File watching via `notify`

### `codegg-protocol`

- `CoreRequest`, `CoreResponse`, `CoreEvent` — daemon/frontend wire types
- `TuiMessage`, `UiNode`, `UiEffect` — plugin UI types
- `PluginManifestDto`, `PluginInvocation`, `PluginResponse`

### `codegg-providers`

- `CircuitBreaker` — provider fault tolerance
- Auth types (credential store, encryption)
- Provider implementations and discovery

### `codegg-git`

- `classify_git()` — risk classification for git operations
- `parse_git_argv()` — typed argv parser
- `GitOperation`, `GitRisk`, `GitFamily` — operation model types
- Pure data crate; no subprocess execution

### `eggsentry`

- `command::classify_bash_command`, `classify_git_subcommand`,
  `classify_tool_call`
- `command::CommandClassification`, `CommandRisk`
- `dependency::detect_dependency_file`,
  `recommended_audit_commands`, `DependencyEcosystem`
- `finding::SecurityFinding`, `SecurityReport`, `Severity`,
  `Confidence`, `SecurityCategory`, `FindingMode`, `FindingSource`
- `profile::ProfileRunner`, `SecurityProfile`, `ProfileConfig`
- `scanner::inspect_file`, `inspect_text`
- `EggsecError { Io, FileTooLarge, Join }` — bridged to `ToolError`

### `eggcontext`

- `TokenizerType::{Cl100kBase, Claude, Gemini, O200kBase}` with
  `for_model`, `multiplier`, `is_approximate`
- `TokenEstimate { tokens, tokenizer, approximate }`
- `estimate_with_provenance(text, model) -> TokenEstimate`
- `estimate_tokens_sync(text, model) -> usize`
- `estimate_tokens(text) -> usize`
- `EggcontextError`

### `egggit`

- `status::RepoStatus` — legacy status
- `status_v2::rich_status() -> RichRepoStatus` — rich structured status
  with `DirtySummary`, `OperationState`, `StatusEntry`
- `diff::{diff_summary, diff_text, file_diff, validate_patch,
  ChangedFile, DiffMode, DiffSummary, FileDiff, PatchValidation}`
- `log::log_commits -> Vec<CommitInfo>`
- `blame::{blame_file, BlameEntry, BlameResult}`
- `refs::{list_branches, list_remotes, list_tags, BranchInfo,
  RemoteInfo, TagInfo}`
- `conflict::{buffer_contains_conflict_markers,
  classify_conflict_code, ConflictReport, ...}`
- `operation_state::{detect_repository_operation_state,
  RepositoryOperationState, RecoveryAction, ...}`
- `worktree::WorktreeInfo`
- `EgggitError { Io, Git, NotARepository, InvalidBaseRef, Join }`

### `egglsp`

Large crate (56 modules). Key public API:

- `LspConfig`, `LspRule` — configuration types
- `LspService::new(config)`, `open_file`, `update_file`, `close_file`,
  `save_file`, `shutdown_all`
- `LspOperations` — go_to_definition, find_references, hover,
  document_symbols, code_actions, code_lens, completion, rename,
  formatting, semantic_tokens
- `DiagnosticsCollector`, `DiagnosticsOutput`
- `LspClient` — transport layer
- `LspError` — comprehensive error enum (20+ variants)
- `LspWorkflowRecipe` — composed multi-step operations (repair hunk,
  review diff, security review, etc.)
- `capability`, `context`, `context_policy`, `context_renderer` —
  evidence and context packing for agent consumption
- `diagnostics`, `doctor`, `health` — observability
- `download`, `launch`, `restart`, `supervisor` — server lifecycle
- `overlay`, `preview_registry` — preview-only edit boundary
- `semantic_context` — reusable semantic queries

### `eggsact` (in-process, not a workspace crate)

Consumed as a direct Rust dependency (`eggsact = "1.1.4"`). The adapter
wraps `eggsact::agent::ToolRegistry` in-process:

- `src/eggsact/adapter.rs` — `EggsactRuntime` owns the registry
- `src/tool/deterministic.rs` — `EggsactTool` generic wrapper,
  `build_eggsact_tools()` factory
- `src/preflight/service.rs` — `PreflightService` for harness-side
  validation

Provenance: `backend = "native"`, `implementation = "eggsact/<tool_name>"`,
`trust = LocalTrusted`.

## Codegg-Side Bridge Files

- `src/lsp/mod.rs` — thin compat shim re-exporting `egglsp` and
  bridging config/error types. New code should prefer direct
  `egglsp::...` imports.
- `src/security/mod.rs` — keeps policy, sandboxing, SSRF in Codegg;
  re-exports `eggsentry` submodules for backward compat.
- `src/worktree/mod.rs` — keeps mutating worktree operations; re-exports
  `list_worktrees` from `egggit`.
- `src/eggsact/adapter.rs` — wraps `eggsact::agent::ToolRegistry` as
  an in-process dependency.

## Test Strategy

- Each crate has self-contained unit tests in `crates/<name>/src/*.rs`.
- Codegg wrapper tests snapshot model-facing JSON schemas.
- `tests/tool_registry.rs` locks down tool surface: required names,
  categories, disabled/missing behavior across backend configs.
- `tests/tool_structured_execution.rs` locks down structured execution
  and definition-visibility contracts.
- Failure paths tested at dispatch layer
  (`tool::backend::report_tests`).
- Integration tests with real subprocesses use `#[tokio::test]`.

```bash
cargo test -p codegg-core
cargo test -p codegg-config
cargo test -p codegg-protocol
cargo test -p codegg-providers
cargo test -p codegg-git
cargo test -p egggit
cargo test -p eggsentry
cargo test -p eggcontext
cargo test -p egglsp --features lsp-test-support
cargo test --test tool_registry
cargo test --test tool_structured_execution
```

## Related Docs

- `architecture/codegg_core.md` — core crate boundary details
- `architecture/tool.md` — tool registry and model-facing contract
- `architecture/lsp.md` — LSP subsystem
- `architecture/deterministic_tools.md` — eggsact tool catalog
