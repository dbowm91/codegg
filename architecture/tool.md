# Tool Module

## Progressive discovery and semantic read surfaces

`tool_search` uses a two-stage contract. Broad queries return bounded,
schema-free descriptors so selecting among large tools does not inflate the
context window. An exact selected name can request `detail: "schema"`; the
response is resolved from the live catalog after the current policy and
hidden-tool checks. The expanded schema is descriptive only; calls still
enter through `ToolBroker`.

`git_query` is the model-facing deferred read facade for bounded status, diff,
log, and branch inspection. It delegates to `GitReadTool`, while `git` retains
typed mutation, recovery, network, and compatibility ownership. No separate
Git execution path or recovery state exists.

`verify` is the bounded semantic verification facade for supported Cargo
workspaces. Its allow-listed actions (`check`, `build`, `lint`, `typecheck`,
`format_check`, and `auto`) generate offline Cargo argv and delegate through
the configured `BashTool`; command-intent classification, scheduler/managed
process routing, sandboxing, audit, and bounded output therefore keep their
existing owners. It accepts no command, package, path, installation, or
auto-fix field. Unsupported project families return a resolver miss.

Session tool construction threads the daemon-resolved LSP service and a
bounded turn-local preview-registry handle through `SessionToolContext` into
`ToolRegistryOptions`. The model-facing `lsp` and hidden `lsp_read` adapters
share the supplied service, and direct-only `lsp_preview_apply` consumes the
same explicit preview handle plus canonical workspace locks and host-bound
session/workspace identity. Its only model input is an opaque preview ID; it
delegates checked mutation to `src/lsp/mutation.rs::apply_preview` and cannot
be called by Tool Programs.

Agent file tools remain disk-authoritative even when an editor has unsaved
text. LSP semantic requests may use a managed editor snapshot, but they do not
redirect tool reads or writes to the editor buffer. A preview apply targeting
a dirty managed document is rejected under the same document operation gate
used by daemon checked save; save and regenerate the preview first.

The `tool` module provides the built-in tools that the agent can use to
interact with the filesystem, shell, and external services. It owns the
tool registry, the execution pipeline, and the backend/diagnostics
abstraction.

## Purpose

- Tool registry management (registration, lookup, filtering, definitions)
- Built-in tool implementations (39 always-registered tools in
  `with_options()`, plus conditional eggsact, evidence, memory, extension,
  todo-read, work-plan, context, and disabled-backend tools)
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
not consume scheduler capacity indefinitely. Run control is the only agent
coordination authority (the legacy file-backed `src/agent/team.rs` inbox was
removed by the residual-runtime-consolidation M001 retirement) and is never
general project chat.

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
├── bash.rs             # BashTool facade: config/builders + high-level
│                       # execute sequence (policy → execution → result);
│                       # re-exports DispatchOutcome and the child-workspace
│                       # validator for terminal.rs compatibility
├── bash/
│   ├── policy.rs       # Canonical model-shell safety/classification seam
│   │                   # plus Bash-owned policy glue (blocked patterns,
│   │                   # blocked/allow lists, child worktree ceiling, kill
│   │                   # switches, intent-family adapters). Terminal calls
│   │                   # the shared safety seam; this module never spawns.
│   ├── process.rs      # Supervised execution: raw-shell spawn via
│   │                   # ManagedProcessService, native/managed dispatch,
│   │                   # scheduler-owned submissions, DispatchOutcome.
│   │                   # No second shell executor; timeout/cancel/reap
│   │                   # owned by the managed service.
│   └── output.rs       # Bounded capture/truncation, routing metadata,
│                       # caller-owned RunStore persistence (delegated
│                       # backends with a run id are skipped), result
│                       # shaping. Persistence failure never rewrites the
│                       # terminal outcome.
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
├── invalid.rs         # Malformed call handler
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
- `multiedit` — historical name only (removed from the registry in M001; the affected-paths reader is retained for stored runs)
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

A logical batch containing a supported native mutation and an unknown or
potentially mutating call (including arbitrary Bash/terminal, Git mutation,
plugin, or MCP work) is non-restorable as a whole; the native subset is not
checkpointed. A native mutation may be accompanied by another call only when
the existing authoritative effect classifier marks that call read-only.

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

The daemon-owned workspace service lease supplies the canonical
`WorkspaceLockTable` to each turn. For an eligible batch,
`ToolBatchExecutor` holds the per-repository guard across pre-capture,
native execution, post-capture, and persistence, so independent sessions
produce a coherent serial checkpoint history without a daemon-global lock.

See `architecture/snapshot.md` for the `EditCheckpoint` storage
contract and `architecture/agent.md` for the `ToolBatchExecutor`
ownership.

## Tool Trait

Defined in `src/tool/mod.rs:157-238`:

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

    fn causal_contract(&self) -> Option<ToolCausalContract> { None }
}
```

`causal_contract()` is additive planning metadata for the causal-frontier
experiment (`crate::tool_advisor::causal_frontier`). A `None` means "not
causally classifiable", never "forbidden"; pilot native tools override it with
`native_causal_contract(...)`.

### ToolCategory

Defined in `src/tool/mod.rs:133-142`, the category drives permission
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

Native surface resolution uses the category reported by each registered
Tool; the catalog retains that semantic metadata for the lifetime of the
shared registry. `tool_category_for_name()` in `src/permission/mod.rs` is a
conservative fallback for paths that only have a name (for example an
external/MCP definition), and falls back to `Mutating` for unknown names.

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

### Convergence actions

`TaskTool` owns the convergence action family so convergence uses the normal
durable task and scheduler boundary. `converge` accepts one bounded producer
request and an M002 single-cycle spec. Producer completion is read from the
authoritative `AgentRunStore`; only a successful structured result can create
the independent read-only `verifier` child. The verifier's host-enforced deny
ceiling covers mutation, shell, Git integration, delegation, permission
responses, and goal completion even when its declarative agent is overridden.

`convergence_status` exposes bounded state, `convergence_cancel` targets only
the referenced active runs through run control, and `convergence_decide` is
owner-authorized and limited to `accept`, `stop`, and `escalate` in M002.
Semantic verification is advisory: it never integrates a worktree or changes
deterministic goal-verification semantics.

The default registry contains product built-in tools; the exact visible
set varies with configuration and optional features. Use the registry
and `tool_search` documentation as the source of truth rather than a
fixed count.

### Registration vs Definitions vs Discovery vs Invocation (M002)

Four questions have four different owners; do not conflate them.

| Question | Owner | Notes |
|---|---|---|
| Registered? | `ToolRegistry::with_options` | Full capability set; includes deferred + hidden stubs. The live `ToolCatalog` shares this lifecycle. |
| Advertised now? | Resolved surface + provider deferral | Immediate definitions for this turn; deferred flagged via `defer_loading`. See `src/tool/disclosure.rs`. |
| Discoverable? | `ToolCatalog` + `tool_search` | Deferred but policy-allowed tools with canonical name, category, risk, disclosure metadata. The catalog is live for late registration, results are deterministic, capped at 10, and empty queries return none. |
| Callable? | `ToolBroker` + permission + contracts | Disclosure never widens authority; denied/hidden tools stay non-callable. |

Operator views (`/tool-backends`, diagnostics) report registered
capability, not just the currently advertised prompt slice. Deferred
tools are not hidden from operators.

### Always-Registered Tools (39)

Registered unconditionally in `with_options()` (`src/tool/mod.rs:408`).
There are 46 unconditional `registry.register(...)` call sites
(`src/tool/mod.rs:512-974`) covering 39 distinct tools: `todowrite` has two
sites (policy branch and no-session fallback), and `lsp`/`security` each have
four sites (native, disabled stub, MCP-with-fallback, MCP-no-fallback) that
always register exactly one variant. A further 17 sites are gated
(see the conditional sections below), and the `for tool in visible/deferred`
loops at `src/tool/mod.rs:935`/`:938` each register many tools, so
`grep -c '\.register(' src/tool/mod.rs` (= 64 sites including the
`catalog.register` call inside `ToolRegistry::register`) is not a tool count.

Disclosure is resolved by `disclosure_for()` in `src/tool/disclosure.rs:61`
(`Core` = ordinary immediate, `Deferred` = discoverable, `Hidden` = never
advertised):

| Tool | Disclosure | File | Description |
|------|------------|------|-------------|
| **bash** | Core | `bash.rs` + `bash/{policy,process,output}.rs` | Shell commands with security (blocked patterns, allowlist, Landlock). 120s timeout. Facade sequences policy → execution → result; see module headers for the pre-spawn order. |
| **verify** | Core | `verify.rs` | Bounded Cargo `check`/`build`/`lint`/`typecheck`/`format_check`/`auto` facade delegating through `BashTool`. Takes no command/package/path field. |
| **read** | Core | `read.rs` | Read file contents with line numbers. Images/PDFs as base64. |
| **write** | Core | `write.rs` | Create or overwrite files with auto-formatting. |
| **edit** | Core | `edit.rs` | Surgical search-and-replace with 8 matching strategies. |
| **glob** | Core | `glob.rs` | Find files matching glob patterns (gitignore-compliant). |
| **grep** | Core | `grep.rs` | Regex content search with bounded workers and path-ordered output. |
| **list** | Core | `list.rs` | Directory tree listing, limited to 300 files. |
| **task** | Core | `task.rs` | Spawn subagents. Supports spawn/get actions. |
| **webfetch** | Core | `webfetch.rs` | URL content fetching via search_backend dispatch. |
| **websearch** | Core | `websearch.rs` | Web search via search_backend dispatch. |
| **research** | Deferred | `research.rs` | Deep research (may invoke websearch/webfetch). Immediate for the `research` role. |
| **image** | Deferred | `image.rs` | DALL-E image generation (dall-e-3, size, quality). |
| **codesearch** | Deferred | `codesearch.rs` | Compatibility alias for coding-focused repo_search (M001 retained); canonical `repo_search` stays core. |
| **question** | Core | `question.rs` | Ask user clarifying questions. |
| **mcp_resource_search** | Deferred | `mcp_resource.rs` | Search MCP-exposed resources. |
| **mcp_resource_read** | Deferred | `mcp_resource.rs` | Read one MCP-exposed resource by URI. |
| **todowrite** | Core | `todo.rs` | Create/update todo items. Always registered: policy-gated with persistence when `todo_state`+`todo_policy` are present, otherwise the canonical in-memory `TodoWriteTool`. |
| **skill** | Core | `skill.rs` | Load a skill (SKILL.md) by name into context. |
| **skill_proposal** | Deferred | `skill_proposal.rs` | Submit one user-authorized portable SKILL.md proposal for preview. `SafeMutating`, `DirectOnly` (agent loop only; subagents and Tool Programs denied), `NonIdempotent`, no retry. Requires an active `/skill-promote` request ID plus matching session/project/habit scope and fresh fingerprint/revision; never writes a skill root. |
| **apply_patch** | Core | `apply_patch.rs` | Apply unified diff patches (update/create/delete/move). |
| **diff** | Core | `diff.rs` | Show differences between two file versions. |
| **replace** | Deferred | `replace.rs` | Regex find/replace with capture groups (`edit`/`apply_patch` stay core). |
| **review** | Deferred | `review.rs` | LLM-based code review with emoji categorization. |
| **terminal** | Deferred | `terminal.rs` | Interactive terminal session (env var filtering). 60s timeout (`bash` stays core). |
| **test** | Core | `test.rs` | Supervised test runner with previous-failures index. Category: ShellExec. |
| **python_script** | Deferred | `src/python_script/tool.rs` | Python script execution (analyze/transform/verify). `bash` stays core. |
| **tool_program** | Deferred | `tool_program.rs` | Foreground model tool for restricted-Python programs. `task` delegation stays core. Contract callability independent of disclosure. |
| **git** | Core | `git.rs` | Git command execution with subcommand/args. 30s timeout. |
| **git_read** | Hidden | `git_read.rs` | Hidden program-only git read adapter (`ProgrammaticOnly`), broker-callable by programs, never model-visible or discoverable. |
| **git_query** | Deferred | `git_read.rs` | Model-facing read facade delegating to `git_read` for bounded status/diff/log/branch inspection. |
| **lsp** | Core, or hidden `DisabledTool` | `lsp.rs` | LSP client tools. Exactly one variant is always registered: native wrapper, or a `DisabledTool` stub when the backend is `disabled` or MCP-configured without fallback. |
| **lsp_read** | Hidden | `lsp_read.rs` | Hidden program-only LSP read adapter, always registered so it fails closed as a typed execution error without a live server. |
| **commit** | Deferred | `commit.rs` | LLM-generated commit messages from diff (`git` stays core). |
| **security** | Deferred, or hidden `DisabledTool` | `security.rs` | Security scanning (wraps eggsentry). Same one-of-two backend logic as `lsp`; immediate for the `security-review` role. |
| **plan_enter** | Core | `plan.rs` | Enter plan mode (reduced toolset). |
| **plan_exit** | Core | `plan.rs` | Exit plan mode. |
| **invalid** | Hidden | `invalid.rs` | Catch-all for malformed tool calls. Registered but never in definitions/discovery. |
| **tool_search** | Core | `tool_search.rs` | On-demand tool discovery via catalog search. Returns canonical name, category, risk, disclosure; capped at 10; empty queries return none. |

### Conditional: Eggsearch Wrappers (7 tools)

Registered only when `[search].backend = "eggsearch"` (evidence enabled).
`repo_search` is core; the rest are deferred with role overrides for
`research`/`security-review`/`verifier` (see `disclosure.rs`):

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
| **lsp** | Native or DisabledTool | LSP client tools. Native when backend is Native/Builtin/fallback-MCP; DisabledTool when disabled or MCP-no-fallback. Core when native. |
| **lsp_preview_apply** | Session-scoped native | Deferred, direct-only checked LSP preview application. Registered only with pool, workspace locks, shared LSP service, and preview handle; unavailable to Tool Programs. |
| **security** | Native or DisabledTool | Security scanning. Same backend logic as LSP. Deferred for ordinary coding; immediate for `security-review` role. |

### Conditional: Todo Tools (1-2 tools)

`todowrite` is always registered (see the table above); only `todoread` is
policy-gated via `TaskStatePolicy`:

| Tool | Condition | Description |
|------|-----------|-------------|
| **todoread** | `allow_model_todo_read` | Read todo items. |
| **todowrite** | `allow_model_todo_write && mode != Disabled` | Policy-gated variant with session persistence. |
| **todowrite** (no session context) | `todo_state` and `todo_policy` both `None` | Canonical `TodoWriteTool` with default in-memory state and the explicit-todo policy (no session persistence). The legacy `TodoTool` duplicate was removed in M001. |

### Conditional: Memory Tools (2 tools)

Registered only when `ToolRegistryOptions.memory_store` is `Some`
(`src/tool/mod.rs:617-626`); scope derives from
`MemoryReadScope::for_project(project_identity)`:

| Tool | Disclosure | Description |
|------|------------|-------------|
| **memory_search** | Deferred | Bounded project-scoped memory search. |
| **memory_get** | Deferred | Bounded project-scoped memory read. |

### Conditional: Extension Tools (2 tools)

Registered only when `ToolRegistryOptions.extension_catalog` is `Some`
(`src/tool/mod.rs:627-634`):

| Tool | Disclosure | Description |
|------|------------|-------------|
| **extension_search** | Deferred | Search the host extension catalog. |
| **extension_install_request** | Deferred | Submit an extension install request (never installs directly). |

### Conditional: WorkPlan Tools (2 tools)

Registered only when both `pool` and `session_id` are `Some`
(`src/tool/mod.rs:675-683`):

| Tool | Disclosure | Description |
|------|------------|-------------|
| **work_plan_get** | Core | Bounded model-facing read of the durable session plan. |
| **work_plan_update_item** | Core | Bounded model-facing per-item plan update. |

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

Manages registration and lookup at `src/tool/mod.rs:249-258`:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: catalog::ToolCatalog,
    tool_backends: ToolBackendConfig,
    integrated_config: IntegratedToolRuntimeConfig,
    search_runtime: SearchRuntimeContext,
    sandbox_profile: SandboxProfile, // M005 resolved profile
    lsp_preview_registry: Option<LspPreviewRegistryHandle>,
    lsp_service: Option<Arc<crate::lsp::service::LspService>>,
}
```

### Key Methods

| Method | Description |
|--------|-------------|
| `new()` | Create empty registry |
| `with_options(ToolRegistryOptions)` | **Authoritative registration sequence** |
| `with_defaults()` | Thin wrapper: `with_options(ToolRegistryOptions::default())` |
| `with_config(&Config)` | Resolves backend + integrated config, calls `with_options` (config-only search context, no MCP handle) |
| `with_config_and_search_runtime(&Config, SearchRuntimeContext)` | Production startup constructor: threads the bootstrapped search/MCP context into every wrapper |
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

Centralizes all knobs that influence registration (`src/tool/mod.rs:276`,
29 fields):

```rust
pub struct ToolRegistryOptions {
    pub tool_advisor: Option<ToolAdvisorConfig>,
    pub todo_state: Option<Arc<Mutex<TodoState>>>,
    pub todo_policy: Option<TaskStatePolicy>,
    pub pool: Option<SqlitePool>,
    pub session_id: Option<String>,
    pub lsp_service: Option<Arc<LspService>>,
    pub document_service: Option<Arc<DocumentService>>,
    pub lsp_preview_registry: Option<LspPreviewRegistryHandle>,
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
    pub child_git_policy: Option<ChildGitPolicy>,
    pub asset_snapshot: Option<Arc<ProjectAssetSnapshot>>,
    pub asset_pin: Option<Arc<Mutex<RuntimeAssetPin>>>,
    pub notification_service: Option<Arc<ToolProgramNotificationService>>,
    pub search_runtime: Option<SearchRuntimeContext>,
    pub sandbox_profile: Option<SandboxProfile>, // M005: None = WorkspaceWrite default
    pub memory_store: Option<Arc<dyn MemoryStore>>,
    pub project_identity: Option<String>,
    pub extension_catalog: Option<Arc<crate::plugin::marketplace::MarketplaceService>>,
}
```

**M005 sandbox wiring:** `with_options()` resolves
`sandbox_profile` (default `WorkspaceWrite`) and configures `BashTool`
via `sandbox_config_for_profile()` over the authoritative
`workspace_root`. Constrained profiles receive an enabled Landlock
config on supported hosts; `FullHost` intentionally carries no
containment and is logged as explicit/auditable. The resolved profile
is stashed on the registry (`ToolRegistry::sandbox_profile()`).
Production turn/session construction (`build_session_tool_registry`,
`DefaultTurnRuntime`) threads the daemon-resolved profile; child
registries narrow it via `resolve_child_sandbox()`. Workspace cwd
validation alone is not OS containment. Guard:
`scripts/check_sandbox_policy_wiring.py`; integration:
`tests/sandbox_policy_wiring.rs`.

The `evidence_config`, `deterministic_config`, and `preflight_config`
fields are resolved by `integrated_config::resolve_integrated_config()`
and passed through from `with_config()`, `with_session_config_defaults()`,
and `build_session_tool_registry()`. `with_defaults()` and
`with_session_defaults()` pass `None` for these (tests only).

The `search_runtime` field (M005) carries the explicit runtime-owned
`SearchRuntimeContext` (owned immutable `SearchConfig` snapshot plus
the shared daemon-owned `McpService` handle). Every search/evidence
wrapper (`websearch`, `webfetch`, `repo_*`, `security_search`,
`research_search`, `batch_fetch`, `evidence_bundle`, `codesearch`) is
constructed with a clone of this context and executes against it;
no wrapper consults the deprecated `search_backend::state`
process-global slots at execution time. `with_options` stores the
context on the registry (`ToolRegistry::search_runtime()`) so
agent-loop capability gates and MCP exposure policy read the same
runtime truth. `with_defaults()` falls back to an isolated default
context (default config, no service). Turn/session construction
(`build_session_tool_registry`, exec/run startup) threads the
bootstrapped context via `SessionToolContext::search_runtime` or
`with_config_and_search_runtime`.

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
- **Deterministic**: `enabled`, `backend`, `profile` (validated by the linked
  eggsact `Profile::from_str_opt()` / `available_profiles()` APIs),
  `model_audience`, `harness_audience`,
  `expose_expert_tools`, `max_output_chars`
- **Preflight**: `enabled`, `mode` (off/observe/warn/block_on_definite),
  `log_findings`, `model_visible_findings`

The resolved config is stashed on `ToolRegistry.integrated_config` and
consumed by `with_options()`, `build_report()`, and subagent
construction (`worker.rs`).

### execute_capture (Central Execution Path)

`ToolRegistry::execute_capture(name, input, ctx)` at
`src/tool/mod.rs:1317-1349` is the central execution path for native
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

Metadata management and search at `src/tool/catalog.rs:170-178`:

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

## REMOVED in M001 (no supported consumer)

**multiedit** (former `src/tool/multiedit.rs`):
- Module deleted; it was never in `ToolRegistry::with_options()`.
- Canonical replacements: `edit` (single edit) or `apply_patch` (batch).
- Coupled live references removed: risk classification, permission-mode
  lists, built-in agent allow/deny entries, child-isolation/read-only
  tool lists, per-tool timeout, and the pre-execution snapshot gate.
- Historical-name readers retained for stored runs: `affected_paths`
  extraction/restorable check, session-import redaction, eggsentry
  classification, workflow-action mapping, and TUI target rendering.

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
4. **Canonical shell blocked patterns**: Bash policy owns regex-based detection of 40+ dangerous patterns; the retained terminal compatibility adapter delegates to it.
5. **Canonical blocked commands**: Bash policy owns the shared HashSet of full commands blocked (rm -rf /, etc.); terminal may add only narrower adapter restrictions.
6. **SSRF protection**: WebFetch validates URLs against internal IPs
7. **Subprocess PATH**: Uses `std::env::var_os("PATH")` (not hardcoded)
8. **Environment filtering**: TerminalTool retains its compatibility-specific filtering of LD_PRELOAD, DYLD_*
9. **Allowlist support**: Bash policy evaluates allowlists for Bash and terminal's effective `sh -c` payload

## Testing

Narrowest test commands:

```bash
cargo test -p codegg --lib tool                         # unit tests
cargo test --test tool_structured_execution              # execute_capture contract
cargo test --test agent_loop_harness::test_live_dispatcher  # dispatcher integration
```

M008 qualification (`tests/reliability_qualification_m008.rs`) proves
idempotent recovery with stable keys, exactly-once uncertain surfacing for
non-idempotent ack loss (secret-safe diagnostics), fake-mutation-backend
reconciliation without replay, and cancellation during backoff.

## Related Docs

- [tool_broker.md](tool_broker.md) — Canonical execution boundary
- [retry.md](retry.md) — Unified retry budget and side-effect reconciliation (M002)
- [deterministic_tools.md](deterministic_tools.md) — Eggsact tools
- [preflight.md](preflight.md) — Harness-side preflight
- [agent.md](agent.md) — Uses ToolRegistry for execution
- [permission.md](permission.md) — Permission checking
- [native_crates.md](native_crates.md) — Backend boundary and provenance

## Source Verification

Verified 2026-10-06 against source. Corrected: always-registered tool count
"~31" → 39 distinct tools from 46 unconditional register sites
(`src/tool/mod.rs:408` `with_options()`, sites `:512-974`), with the
register-site vs distinct-tool explanation stated inline; the core table gained
the 8 missing always-registered tools (`verify`, `mcp_resource_search`,
`mcp_resource_read`, `todowrite`, `git_read`, `git_query`, `lsp_read`, and
explicit hidden/deferred disclosure for `git_read`/`lsp_read`); added missing
conditional families Memory (2), Extension (2), WorkPlan (2); todo section
corrected to 1-2 tools. Line refs: `Tool` `136-205` → `156-237`,
`ToolCategory` `115-132` → `132-141`, `ToolRegistry` `215-221` → `248-257`,
`execute_capture` `1036-1068` → `1249-1281`. `ToolRegistry` 6 → 8 fields;
`ToolRegistryOptions` 22 → 29 fields; `Tool` gained `causal_contract`.
Verified accurate: 64 `.register(` sites in `src/tool/mod.rs` (63 tool
registrations + 1 `catalog.register`), 7 eggsearch wrappers, deterministic
13 = 8 always-visible + 5 deferred (`build_eggsact_tools`,
`src/tool/deterministic.rs:106`), 4 `ToolCategory` variants,
`ToolResult` 4 fields, `ToolCatalog` at `src/tool/catalog.rs:170`,
`python_script` name (`src/python_script/tool.rs:444`).

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"), which rewrote `src/tool/mod.rs` to carry
the resolved `DecisionEngine` and left `ToolRegistryOptions.runtime_backend`
selection explicit. Every `src/tool/mod.rs` line ref in this document was
re-derived against the new file. Corrected: `ToolCategory` `132-141`->`133-142`,
`trait Tool` `156-237`->`157-238`, `ToolRegistry` `248-257`->`249-258`,
`ToolRegistryOptions` `272`->`276`, `with_options()` `392`->`408`, the
registration span `468-924`->`512-974`, the two registration loops
`891`/`894`->`935`/`938`, the memory-store block `573-582`->`617-626`, the
extension-catalog block `583-590`->`627-634`, the WorkPlan pool/session block
`631-639`->`675-683`, and `execute_capture` `1249-1281`->`1317-1349`. Each target
was re-checked to land on the same construct it previously named. Confirmed
unchanged by that commit: the disclosed tool set, the
`off`/`observe`/`rerank`/`promote` semantics, and every tool name in the tables.
