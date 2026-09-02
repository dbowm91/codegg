# Tool Module

The `tool` module provides the built-in tools that the agent can use to
interact with the filesystem, shell, and external services. It owns the
tool registry, the execution pipeline, and the backend/diagnostics
abstraction.

## Purpose

- Tool registry management (registration, lookup, filtering, definitions)
- Built-in tool implementations (~31 always-registered core tools plus
  conditional eggsact, evidence, and context tools)
- Tool execution with permission checking, structured provenance, and
  backend-aware diagnostics
- Backend abstraction (native, MCP, shell, builtin legacy) via
  `ToolBackendConfig`
- On-demand tool discovery via `ToolCatalog` and `tool_search`

The `task` tool is the compatibility surface for durable delegated runs. In
addition to `spawn`, it accepts `status` (`get` is retained), `message`,
`interrupt`, `wait`, and `cancel`. These operations address a typed durable
run ID, enforce owner/ancestor lineage, and use bounded payloads. `wait` is a
bounded long-poll; a timeout reports that the run is still active and does
not consume scheduler capacity indefinitely. Run control is distinct from
the file-backed team inbox and is never general project chat.

For concurrent work, `spawn_many` and run-group actions provide bounded fan-out
and deterministic joins. New callers should prefer typed `AgentRunId` values,
`wait`/`wait_group`, and push notifications; `status`/`get` remain compatibility
inspection paths for older clients. Child completion still requires explicit
parent-side integration when a worktree produced a commit.

## Where It Lives

```
src/tool/
├── mod.rs              # Tool trait, ToolRegistry, with_options()
├── backend.rs          # ToolBackendKind, ToolProvenance, StructuredToolResult,
│                       # ToolBackendConfig, build_report() for /tool-backends
├── backend_config.rs   # ToolBackendConfig::from_config()
├── integrated_config.rs# IntegratedToolRuntimeConfig: resolve_integrated_config()
├── factory.rs          # build_session_tool_registry()
├── catalog.rs          # ToolCatalog for metadata and search (BM25 + keyword)
├── broker.rs           # ToolBroker (see tool_broker.md)
├── contract.rs         # ToolContract, ToolCallerPolicy, ToolValue
├── util.rs             # Path validation helpers
├── disabled.rs         # DisabledTool stub for hidden/disabled backends
├── bash.rs             # Shell command execution
├── read.rs             # File reading with image/PDF base64 support
├── write.rs            # File writing with auto-formatting
├── edit.rs             # 8-strategy edit matching
├── glob.rs             # Glob pattern file finding
├── grep.rs             # Regex content search
├── list.rs             # Directory tree listing
├── diff.rs             # Unified diff generation
├── replace.rs          # Regex find/replace
├── apply_patch.rs      # Unified diff patch application
├── patch_util.rs       # Shared patch utilities for apply_patch and LSP preview
├── task.rs             # Subagent task spawning
├── todo.rs             # Todo list management (todowrite/todoread)
├── webfetch.rs         # URL content fetching (dispatches to search_backend)
├── websearch.rs        # Web search (dispatches to search_backend)
├── repo_search.rs      # Repository search (eggsearch wrapper)
├── repo_fetch.rs       # Repository file fetch (eggsearch wrapper)
├── repo_map.rs         # Repository directory map (eggsearch wrapper)
├── security_search.rs  # Security advisory search (eggsearch wrapper)
├── research_search.rs  # Academic/research search (eggsearch wrapper)
├── batch_fetch.rs      # Batch URL fetch (eggsearch wrapper)
├── evidence_bundle.rs  # Evidence bundle builder (eggsearch wrapper)
├── codesearch.rs       # Coding-focused repo_search compatibility alias
├── question.rs         # User question asking
├── skill.rs            # Skill loading
├── review.rs           # LLM-based code review
├── batch.rs            # Parallel tool execution
├── terminal.rs         # Terminal command execution
├── test.rs             # Supervised test runner
├── git.rs              # Git command execution (low-level wrapper)
├── commit.rs           # LLM-generated commit messages
├── plan.rs             # plan_enter and plan_exit tools
├── invalid.rs          # Malformed call handler
├── multiedit.rs        # Multi-edit tool (NOT registered by default)
├── image.rs            # DALL-E image generation
├── tool_search.rs      # On-demand tool discovery
├── lsp.rs              # LSP client tools (wraps egglsp::LspService)
├── security.rs         # Security scanning (wraps eggsentry)
├── deterministic.rs    # EggsactTool wrapper, build_eggsact_tools()
└── ...
```

## Mutation Surface and Edit Checkpoints

The durable restorable edit surface is the native file-mutating tool
set handled by `ToolBatchExecutor` (`src/agent/tool_batch.rs`) and
`crates/codegg-core/src/snapshot/affected_paths.rs`:

- `write` — one target path; absent→present or present→present
- `edit` — one existing path
- `replace` — one existing path
- `multiedit` — one existing path (multiple sequential edits)
- `apply_patch update` — one existing path
- `apply_patch create` — one target path (normally absent→present)
- `apply_patch delete` — one existing path (present→absent)
- `apply_patch move` — both source and destination (dest pre-state
  included if replacement permitted)

All other mutations (bash/shell arbitrary commands, plugin/MCP
filesystem writes, git commits/branch ops, package-manager or DB side
effects, binary content beyond safe snapshot UTF-8 handling) are
**explicitly non-restorable** and are never implicitly treated as
safely captured. A batch containing only non-restorable tools produces
no edit checkpoint; a malformed move/create/delete that cannot be
safely derived marks the batch non-restorable rather than persisting a
partial checkpoint.

Checkpoints distinguish `Absent` vs `Present { hash, content }` so
create/delete/move are representable without empty-file equivalence.
Every checkpoint is scoped to explicit
`workspace_id`/`session_id`/`turn_id`/`batch_seq` and validated with
the same `SnapshotOptions` bounds and `is_safe_relative_path`/symlink
checks as snapshots. Oversized or unsafe paths fail the batch
predictably and do not fabricate successful post-state.

`ToolBatchExecutor` derives the complete affected path set from
accepted structured arguments *before* execution, captures pre-state,
executes tools (overlapping paths within a batch serialize to
`effective_max = 1` so pre/post ordering is deterministic), captures
post-state from the same path set after execution, and persists the
checkpoint only when the resulting state meaningfully represents the
mutation. Durable capture no longer depends on drained global
`FileChanged` events; those events remain observational for TUI diff
notification (`AppEvent::FileChanged` → TUI `file_diff.rs`).

See `architecture/snapshot.md` for the `EditCheckpoint` storage
contract and `architecture/agent.md` for the `ToolBatchExecutor`
ownership.

## Tool Trait

Defined in `src/tool/mod.rs:132-201`:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;

    fn category(&self) -> ToolCategory { ToolCategory::Mutating }
    fn set_available_tools(&mut self, _tools: Vec<String>) {}
    fn defer_loading(&self) -> bool { false }
    fn expose_in_definitions(&self) -> bool { true }
    fn has_functional_backend(&self) -> bool { true }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> { ... }

    fn contract(&self, tool_name: &str, input_schema: serde_json::Value)
        -> ToolContract { ToolContract::legacy(tool_name, input_schema) }
}
```

### ToolCategory

Defined in `src/tool/mod.rs:113-130`, the category drives permission
gating and plan-mode filtering:

```rust
pub enum ToolCategory {
    ReadOnly,      // never prompts (read, glob, grep, list, etc.)
    SafeMutating,  // never prompts (todowrite, question, invalid)
    Mutating,      // normal Ask/Allow path (edit, write, git, etc.)
    ShellExec,     // routed to destructive-pattern fallback (bash)
}

impl ToolCategory {
    pub fn is_permission_free(self) -> bool {
        matches!(self, ToolCategory::ReadOnly | ToolCategory::SafeMutating)
    }
}
```

The lookup helper `tool_category_for_name()` in `src/permission/mod.rs:99`
maps a tool name to a category for the permission checker, falling back
to `Mutating` for unknown tools.

### ToolResult

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    pub success: bool,
}
```

## Built-in Tools

The default registry contains product built-in tools; the exact visible
set varies with configuration and optional features. Use the registry
and `tool_search` documentation as the source of truth rather than a
fixed count.

### Always-Registered Core Tools (~31)

Registered unconditionally in `with_options()`:

| Tool | File | Description |
|------|------|-------------|
| **bash** | `bash.rs` | Shell commands with security (blocked patterns, allowlist, Landlock). 120s timeout. |
| **read** | `read.rs` | Read file contents with line numbers. Images/PDFs as base64. |
| **write** | `write.rs` | Create or overwrite files with auto-formatting. |
| **edit** | `edit.rs` | Surgical search-and-replace with 8 matching strategies. |
| **glob** | `glob.rs` | Find files matching glob patterns (gitignore-compliant). |
| **grep** | `grep.rs` | Regex content search with bounded workers and path-ordered output. |
| **list** | `list.rs` | Directory tree listing, limited to 300 files. |
| **task** | `task.rs` | Spawn subagents. Supports spawn/get actions. |
| **webfetch** | `webfetch.rs` | URL content fetching via search_backend dispatch. |
| **websearch** | `websearch.rs` | Web search via search_backend dispatch. |
| **research** | `research.rs` | Deep research (may invoke websearch/webfetch). |
| **image** | `image.rs` | DALL-E image generation (dall-e-3, size, quality). |
| **codesearch** | `codesearch.rs` | Compatibility alias for coding-focused repo_search. |
| **question** | `question.rs` | Ask user clarifying questions. |
| **skill** | `skill.rs` | Load a skill (SKILL.md) by name into context. |
| **apply_patch** | `apply_patch.rs` | Apply unified diff patches (update/create/delete/move). |
| **diff** | `diff.rs` | Show differences between two file versions. |
| **replace** | `replace.rs` | Regex find/replace with capture groups. |
| **review** | `review.rs` | LLM-based code review with emoji categorization. |
| **terminal** | `terminal.rs` | Interactive terminal session (env var filtering). 60s timeout. |
| **test** | `test.rs` | Supervised test runner with previous-failures index. Category: ShellExec. |
| **python_script** | (python_script/) | Python script execution (analyze/transform/verify). |
| **tool_program** | `tool_program.rs` | Foreground model tool for restricted-Python programs. |
| **git** | `git.rs` | Git command execution with subcommand/args. 30s timeout. |
| **commit** | `commit.rs` | LLM-generated commit messages from diff. |
| **plan_enter** | `plan.rs` | Enter plan mode (reduced toolset). |
| **plan_exit** | `plan.rs` | Exit plan mode. |
| **invalid** | `invalid.rs` | Catch-all for malformed tool calls. |
| **tool_search** | `tool_search.rs` | On-demand tool discovery via catalog search. |

### Conditional: Eggsearch Wrappers (7 tools)

Registered only when `[search].backend = "eggsearch"` (evidence enabled):

| Tool | File | Description |
|------|------|-------------|
| **repo_search** | `repo_search.rs` | Search repositories via eggsearch. |
| **repo_fetch** | `repo_fetch.rs` | Fetch repository file content via eggsearch. |
| **repo_map** | `repo_map.rs` | Get repository directory structure. |
| **security_search** | `security_search.rs` | Search security advisories (CVE/GHSA/OSV). |
| **research_search** | `research_search.rs` | Search academic/research sources. |
| **batch_fetch** | `batch_fetch.rs` | Fetch tagged web or repository items. |
| **evidence_bundle** | `evidence_bundle.rs` | Build evidence bundles from source-cards. |

`websearch` and `webfetch` always present stable native tool names.
Raw `mcp__eggsearch__*` tools hidden by default
(`expose_raw_mcp_tools = false`).

### Conditional: LSP/Security Backend Tools (2-4 tools)

| Tool | Registration | Description |
|------|-------------|-------------|
| **lsp** | Native or DisabledTool | LSP client tools. Native when backend is Native/Builtin/fallback-MCP; DisabledTool when disabled or MCP-no-fallback. |
| **security** | Native or DisabledTool | Security scanning. Same backend logic as LSP. |

### Conditional: Todo Tools (0-2 tools)

Policy-gated via `TaskStatePolicy`:

| Tool | Condition | Description |
|------|-----------|-------------|
| **todowrite** | `allow_model_todo_write && mode != Disabled` | Create/update todo items with priority/status. |
| **todoread** | `allow_model_todo_read` | Read todo items. |
| **todo** (legacy) | No session context | Combined read/write todo tool. |

### Conditional: Context Read (0-1 tool)

| Tool | Condition | Description |
|------|-----------|-------------|
| **context_read** | conditional | Expand compressed tool output via `ctx://` handles. |

### Conditional: Deterministic Tools (13 tools)

Registered when `[deterministic_tools].enabled = true` and
`backend != "disabled"`. See [deterministic_tools.md](deterministic_tools.md).

**Always-visible (8):** text_equal, text_diff_explain,
text_replace_check, validate_json, validate_toml, command_preflight,
path_normalize, text_security_inspect.

**Deferred (5):** text_inspect, config_preflight, identifier_inspect,
structured_data_compare, text_fingerprint.

## ToolRegistry

Manages registration and lookup at `src/tool/mod.rs:212-217`:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: catalog::ToolCatalog,
    tool_backends: ToolBackendConfig,
    integrated_config: IntegratedToolRuntimeConfig,
}
```

### Key Methods

| Method | Description |
|--------|-------------|
| `new()` | Create empty registry |
| `with_options(ToolRegistryOptions)` | **Authoritative registration sequence** |
| `with_defaults()` | Thin wrapper: `with_options(ToolRegistryOptions::default())` |
| `with_config(&Config)` | Resolves backend + integrated config, calls `with_options` |
| `with_session_config_defaults(Config, state, policy, pool, sid)` | **Production session constructor.** Resolves both configs. |
| `with_session_defaults(todo_state, policy, pool, session_id)` | Drops loaded `[tool_backends]` — tests only. |
| `register(&mut self, tool: impl Tool + 'static)` | Register a tool (takes owned value) |
| `get(&self, name: &str) -> Option<&dyn Tool>` | Get tool by name (includes hidden stubs) |
| `list(&self) -> Vec<&dyn Tool>` | List all tools (includes hidden stubs) |
| `filter_out(&mut self, denied_tools: &[String])` | Remove denied tools |
| `definitions(&self) -> Vec<ToolDefinition>` | Tool definitions for LLM — filters via `expose_in_definitions()` |
| `catalog(&self) -> &ToolCatalog` | Access the tool catalog |
| `execute_capture(name, input, ctx) -> StructuredToolResult` | Central execution path. Returns structured provenance. |
| `tool_backends()` | Resolved `ToolBackendConfig` |
| `integrated_config()` | Resolved `IntegratedToolRuntimeConfig` |
| `backend_report(mcp_server_names)` | Runtime-aware status for `/tool-backends` |

### ToolRegistryOptions

Centralizes all knobs that influence registration. Key fields:

```rust
pub struct ToolRegistryOptions {
    pub todo_state: Option<Arc<Mutex<TodoState>>>,
    pub todo_policy: Option<TaskStatePolicy>,
    pub pool: Option<SqlitePool>,
    pub session_id: Option<String>,
    pub lsp_service: Option<Arc<LspService>>,
    pub tool_backends: ToolBackendConfig,
    pub context_artifact_store: Option<Arc<dyn ContextArtifactStore>>,
    pub context_session_id: Option<String>,
    pub context_read_enabled: bool,
    pub lsp_cache_config: Option<LspCacheConfig>,
    pub evidence_config: Option<EvidenceBackendRuntimeConfig>,
    pub deterministic_config: Option<DeterministicToolsRuntimeConfig>,
    pub preflight_config: Option<PreflightRuntimeConfig>,
    pub run_store: Option<Arc<dyn RunStore>>,
    pub submission: Option<Arc<JobSubmissionService>>,
    pub command_intent: Option<CommandIntentConfig>,
    pub workspace_root: Option<PathBuf>,
    pub asset_snapshot: Option<Arc<ProjectAssetSnapshot>>,
    pub asset_pin: Option<Arc<Mutex<RuntimeAssetPin>>>,
    pub notification_service: Option<Arc<ToolProgramNotificationService>>,
}
```

The `evidence_config`, `deterministic_config`, and `preflight_config`
fields are resolved by `integrated_config::resolve_integrated_config()`
and passed through from `with_config()`, `with_session_config_defaults()`,
and `build_session_tool_registry()`. `with_defaults()` and
`with_session_defaults()` pass `None` for these (tests only).

### Integrated Tool Runtime Config

`src/tool/integrated_config.rs` resolves evidence, deterministic, and
preflight runtime configs from loaded `Config` in one pass:

```rust
pub struct IntegratedToolRuntimeConfig {
    pub evidence: Option<EvidenceBackendRuntimeConfig>,
    pub deterministic: Option<DeterministicToolsRuntimeConfig>,
    pub preflight: Option<PreflightRuntimeConfig>,
}
```

Entry point: `resolve_integrated_config(&Config) -> IntegratedToolRuntimeConfig`.

- **Evidence**: `search_backend`, `expose_raw_mcp_tools`, `fallback_to_builtin`
- **Deterministic**: `enabled`, `backend`, `profile` (validated against
  `KNOWN_EGGSACT_PROFILES`: `codegg_core`, `codegg_core_min`,
  `default`, `full`), `model_audience`, `harness_audience`,
  `expose_expert_tools`, `max_output_chars`
- **Preflight**: `enabled`, `mode` (off/observe/warn/block_on_definite),
  `log_findings`, `model_visible_findings`

The resolved config is stashed on `ToolRegistry.integrated_config` and
consumed by `with_options()`, `build_report()`, and subagent
construction (`worker.rs`).

### execute_capture (Central Execution Path)

`ToolRegistry::execute_capture(name, input, ctx)` at
`src/tool/mod.rs:833-865` is the central execution path for native
tools. It calls `Tool::execute_structured()` internally, populates
a fallback `ToolProvenance::legacy(...)` for tools that do not override
it, and records provenance via `tracing::debug!`. The returned
`StructuredToolResult` is collapsed to `structured.output` for the
model — identical to the legacy `execute()` path.

MCP tools (`mcp__server__tool`) dispatch through
`McpService::call_tool` and are not funnelled through `execute_capture`.

### `expose_in_definitions` Filtering

`Tool::expose_in_definitions()` (default `true`) is the model-facing
predicate. `DisabledTool` overrides to `false`, so
`ToolRegistry::definitions()` and `AgentLoop::build_tool_definitions()`
filter it out of the model-visible catalog. The stub remains registered
and callable by name for diagnostics.

## ToolCatalog

Metadata management and search at `src/tool/catalog.rs:134-143`:

```rust
pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
    search_mode: SearchMode,
    avg_doc_length: f64,
    doc_count: usize,
    idf_cache: HashMap<String, f64>,
}
```

`ToolCatalog::register()` takes `&dyn Tool` (not `Box<dyn Tool>`).

Search supports keyword (case-insensitive substring) and BM25 ranking
modes. BM25 caches are recomputed on each registration when active.

## Tool Backend Diagnostics

`/tool-backends` (aliases `/tools`, `/backends`) surfaces the native vs
MCP wiring of every model-facing tool. The handler builds a report from
the resolved `ToolBackendConfig` plus `IntegratedToolRuntimeConfig`.
See `backend.rs` for `build_report()`.

Status values: `ready`, `disabled`, `unavailable`, `error(<msg>)`.

## NOT Registered (exists but excluded)

**multiedit** (`src/tool/multiedit.rs`):
- Module exists and is registered via `pub mod multiedit` in `mod.rs`
- NOT included in `ToolRegistry::with_options()`
- Applies multiple edit operations to a single file sequentially

## Path Validation

All file operations use utility functions from `src/tool/util.rs`:

- **`validate_path(path, allowed_root)`**: Symlink check + canonical
  root enforcement.
- **`canonicalize_path(path)`**: Symlink check + canonicalize.
- **`check_path_for_symlinks(path)`**: Walks components, rejects
  any symlink.

Key invariants:
- Symlinks rejected at every component
- Allowed root enforcement via canonical prefix check
- All file I/O in `tokio::task::spawn_blocking()`

## ToolContracts and the Canonical Broker

All production tool calls route through `ToolBroker` (`src/tool/broker.rs`),
which enforces a 10-step policy pipeline. Each tool has a `ToolContract`
describing caller policy, effect class, idempotency, retry/cache, and
projection policy. See [tool_broker.md](tool_broker.md).

## Security Considerations

1. **Path validation**: All file paths validated before access
2. **Symlink protection**: `check_path_for_symlinks()` rejects symlinks
3. **Permission enforcement**: Tools check permissions before execution
4. **BashTool blocked patterns**: Regex-based detection of 40+ dangerous patterns
5. **BashTool blocked commands**: HashSet of full commands blocked (rm -rf /, etc.)
6. **SSRF protection**: WebFetch validates URLs against internal IPs
7. **Subprocess PATH**: Uses `std::env::var_os("PATH")` (not hardcoded)
8. **Environment filtering**: TerminalTool filters LD_PRELOAD, DYLD_*
9. **Allowlist support**: BashTool and TerminalTool support command allowlists

## Testing

Narrowest test commands:

```bash
cargo test -p codegg --lib tool                         # unit tests
cargo test --test tool_structured_execution              # execute_capture contract
cargo test --test agent_loop_harness::test_live_dispatcher  # dispatcher integration
```

## Related Docs

- [tool_broker.md](tool_broker.md) — Canonical execution boundary
- [deterministic_tools.md](deterministic_tools.md) — Eggsact tools
- [preflight.md](preflight.md) — Harness-side preflight
- [agent.md](agent.md) — Uses ToolRegistry for execution
- [permission.md](permission.md) — Permission checking
- [native_crates.md](native_crates.md) — Backend boundary and provenance
