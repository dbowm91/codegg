# Tool Module

The `tool` module provides the built-in tools that the agent can use to interact with the filesystem, shell, and external services.

## Overview

**Location**: `src/tool/`

**Key Responsibilities**:
- Tool registry management
- Built-in tool implementations (40 tools in `with_options()`)
- Tool execution with permission checking
- Parameter validation
- On-demand tool discovery via ToolCatalog
- Backend abstraction (native, MCP, shell, builtin legacy) — see `src/tool/backend.rs` and `architecture/native_crates.md`

## Tool Trait

All tools implement the `Tool` trait defined at `src/tool/mod.rs:100-155`:

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
    /// Whether this tool should appear in the model-facing tool
    /// definitions (default `true`). Overridden by `DisabledTool`
    /// to `false` so hidden stubs do not pollute the model tool
    /// surface.
    fn expose_in_definitions(&self) -> bool { true }

    // Optional structured execution — default wraps `execute()`.
    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> { ... }
}
```

**Important Notes**:
- Tools receive only `serde_json::Value` as input (no `ToolContext` struct) by default
- `ToolCatalog::register()` takes `&dyn Tool` (not `Box<dyn Tool>`) - a common oversight
- Every `Tool` reports a `ToolCategory` via `fn category(&self) -> ToolCategory` (default `Mutating`)
- `execute_structured` is opt-in — new wrappers may use it; existing tools keep the default impl
- `expose_in_definitions` is opt-out — hidden stubs (`DisabledTool` for `disabled` or `mcp + fallback_to_native = false`) override it to `false` and rely on the registry/agent loop filtering step to keep them out of the model-facing catalog while remaining callable by name for diagnostics

### ToolCategory

Defined in `src/tool/mod.rs`, the category drives permission gating and
which tools survive `filter_tools_for_model()` (plan mode):

```rust
pub enum ToolCategory {
    ReadOnly,       // never prompts (read, glob, grep, list, webfetch, lsp, diff, plan_*, ...)
    SafeMutating,   // never prompts (todowrite, todoread, question, invalid)
    Mutating,       // normal Ask/Allow path (edit, write, apply_patch, replace, image, terminal, git, commit, review, task, ...)
    ShellExec,      // routed to destructive-pattern fallback (bash, ...)
}

impl ToolCategory {
    pub fn is_permission_free(&self) -> bool {
        matches!(self, Self::ReadOnly | Self::SafeMutating)
    }
}
```

The lookup helper `tool_category_for_name()` in `src/permission/mod.rs`
maps a tool name to a category for the permission checker, falling back
to `Mutating` for unknown tools. This means the permission flow
short-circuits to `Allow` for read-only / safe-mutating tools before
any store / rule / glob check (a persistent `Deny` still wins), and
shell-exec tools get the destructive-pattern fallback described in
[permission.md](permission.md#toolcategory--permission-free-tools).

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

## Built-in Tools (40 total in default registry)

### File Operations

| Tool | File | Description |
|------|------|-------------|
| **read** | `read.rs` | Read file contents with line numbers. Images/PDFs returned as base64. Supports offset/limit. |
| **write** | `write.rs` | Create or overwrite files. Runs auto-formatting after write if configured. |
| **edit** | `edit.rs` | Surgically search and replace text with 8 matching strategies (exact, line-trimmed, whitespace-normalized, block-anchored, indentation-flexible, escape-normalized, trimmed-boundary, context-aware). |
| **glob** | `glob.rs` | Find files matching glob patterns. Uses `ignore` crate for gitignore compliance. |
| **grep** | `grep.rs` | Search file contents using regular expressions with context lines. Concurrent search with semaphore limiting (max 100 concurrent). |
| **list** | `list.rs` | List directory tree with ignore patterns. Limited to 300 files by default. |
| **diff** | `diff.rs` | Show differences between two file versions. Supports unified diff format and line ranges. |
| **replace** | `replace.rs` | Find and replace using regex. Replaces all occurrences by default. Supports capture groups ($1, $2). |
| **apply_patch** | `apply_patch.rs` | Apply unified diff patches. Supports update, create, delete, and move modes. |

### Shell Execution

| Tool | File | Description |
|------|------|-------------|
| **bash** | `bash.rs` | Execute shell commands with extensive security (blocked commands, blocked patterns regex, allowlist support, Landlock sandboxing). 120s default timeout. Phase 04 adds command intent routing metadata: when `CommandIntentConfig` is set, classifies commands via `classify_command()`, plans via `plan_execution()`, resolves via `resolve_routing()`, and appends `[intent: X | backend: Y | projector: Z | confidence: C | risk: R | routing: enabled/disabled | rtk: eligible/off | route: RoutingDecision]` to output. All commands still execute via raw shell; metadata is for visibility and future structured routing. |
| **terminal** | `terminal.rs` | Run commands in interactive terminal session. Similar security to bash but with env var filtering. 60s default timeout. |
| **test** | `test.rs` | Run project tests through supervised test runner. Wraps `test_runner::resolve_and_run_test()`. Streams stdout/stderr to logs, classifies timeouts/failures, returns compact report. Supports `previous_failures` scope which reruns the most recent failing test from a bounded local index (`.codegg/test-runs/index.json`, max 100 entries). Custom commands are validated as argv-prefix matches against a 12-entry allowlist (`cargo test`, `cargo nextest`, `pytest`, `uv run pytest`, `go test`, `zig build test`, `make test`, `make check`, `npm test`, `pnpm test`, `yarn test`, `bun test`). Shell metacharacters, redirection, pipes, command substitution, newlines, and prefix collisions are rejected. The validator is shared with the `/test` slash command and is re-run by the resolver as defense-in-depth. Both generated and custom commands execute via direct `Command::new(argv[0]).args(&argv[1..])` — never via a shell. Category: ShellExec. When session context is available, the test tool can publish lifecycle events (started/progress/completed) through an optional TestEventSink. Events are throttled and do not include raw output. Full test output remains in `.codegg/test-runs/` log directories. |
| **git** | `git.rs` | Execute git commands with subcommand/args model. 30s default timeout. |
| **commit** | `commit.rs` | Generate commit messages from diff using LLM. Stages all changes, generates message, commits with optional Co-Authored-By. |

### Web Operations

| Tool | File | Description |
|------|------|-------------|
| **webfetch** | `webfetch.rs` | Native wrapper. Dispatches to the configured backend via `search_backend::dispatch_web_fetch`. Default backend is the external `eggsearch` MCP server's `web_fetch` tool; legacy reqwest/html2text implementation is retained as the `builtin` fallback. |
| **websearch** | `websearch.rs` | Native wrapper. Dispatches to the configured backend via `search_backend::dispatch_web_search`. Default backend is the external `eggsearch` MCP server's `web_search` tool; the in-tree `SearchProviderRegistry` is the `builtin` fallback. |
| **codesearch** | `codesearch.rs` | Search for code examples, library docs, SDK patterns using Exa Code API. Uses EXA_API_KEY or EXA_CODE_API_KEY. |
| **research** | `research.rs` | Deep research tool. May invoke `websearch` and `webfetch` internally. |
| **image** | `image.rs` | Generate images using OpenAI's DALL-E model. Supports dall-e-3, size, quality parameters. Requires OPENAI_API_KEY. |

### Eggsearch Wrapper Tools

Native wrappers for additional eggsearch MCP tools. These dispatch
through `search_backend` with `backend = "eggsearch"` only (no builtin
fallback). Raw `mcp__eggsearch__*` equivalents are hidden by default.

| Tool | File | Description |
|------|------|-------------|
| **repo_search** | `repo_search.rs` | Search repositories via eggsearch. Wraps `repo_search` MCP tool. |
| **repo_fetch** | `repo_fetch.rs` | Fetch repository file content via eggsearch. Wraps `repo_fetch` MCP tool. |
| **repo_map** | `repo_map.rs` | Get repository directory structure via eggsearch. Wraps `repo_map` MCP tool. |
| **security_search** | `security_search.rs` | Search security advisories via eggsearch. Wraps `security_search` MCP tool. |
| **research_search** | `research_search.rs` | Search academic/research sources via eggsearch. Wraps `research_search` MCP tool. |
| **batch_fetch** | `batch_fetch.rs` | Fetch multiple URLs in parallel via eggsearch. Wraps `batch_fetch` MCP tool. |
| **evidence_bundle** | `evidence_bundle.rs` | Build evidence bundles from multiple sources via eggsearch. Wraps `build_evidence_bundle` MCP tool. |

`websearch` and `webfetch` always present the stable native tool
names to the model. The raw `mcp__eggsearch__*` tools are hidden
from the model by default (`expose_raw_mcp_tools = false`). Set
that flag to `true` to expose them. See
`architecture/search_backend.md` for
the dispatch logic, config schema, and trust framing.

### Task Management

| Tool | File | Description |
|------|------|-------------|
| **task** | `task.rs` | Spawn subagents to handle tasks independently. Supports spawn/get actions. Uses TaskStore for persistence. |
| **todowrite** | `todo.rs` | Create, update, and manage todo items with persistent state. Supports priority (low/medium/high) and status (pending/in_progress/completed). |

### Planning

| Tool | File | Description |
|------|------|-------------|
| **plan_enter** | `plan.rs` | Enter plan mode. Toolset is reduced to read-only + `todowrite` + `bash`; bash is auto-rejected unless it matches the destructive-pattern allowlist (only safe commands). |
| **plan_exit** | `plan.rs` | Exit plan mode and switch to build agent. Optionally specify plan file. |

### User Interaction

| Tool | File | Description |
|------|------|-------------|
| **question** | `question.rs` | Ask user clarifying questions. Returns answers to continue agent loop. Supports options and initial values. |
| **skill** | `skill.rs` | Load a skill (SKILL.md) by name into context. Returns skill content and list of resource files. |

### Code Operations

| Tool | File | Description |
|------|------|-------------|
| **review** | `review.rs` | Analyze git diff and provide structured code review feedback using LLM. Uses emojis for categorization (bug, performance, style, suggestion). |
| **lsp** | `lsp.rs` | Query LSP server for code intelligence and preview-only semantic edits. Operations: goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol, diagnostics, renamePreview, formatPreview, sourceActionPreview (currently only `source.organizeImports`), semanticCheckPreview (accepts `content` or a single-file unified diff `patch`; patch input is applied in memory against `file_path`, never written to disk; collects diagnostics + symbols, restores disk content; multi-file patches unsupported; operation-level root enforcement via `allowed_root`; error fields: `diagnostics_error`, `symbols_error`, `restore_error`). semanticContext (compact pre-edit/pre-review context packet combining source excerpt, diagnostics, symbols, optional definitions/references/overlay, optional call/type hierarchy, optional source-action hints; read-only, bounded sections; per-section errors via `definitions_error`, `references_error`; overlay limits tracked by `overlay_diagnostics_truncated`; `result_count` includes overlay items; source excerpt truncation is UTF-8-safe via char-boundary cutting; accepts `include_source_actions` bool to append source-action hints — currently `source.organizeImports` — as `SemanticSourceActionHint` objects with `action`, `available`, `preview`, `error`; source-action failures do not fail the whole packet; accepts `include_call_hierarchy` and `include_type_hierarchy` bools to append hierarchy context, requiring line+column). callHierarchy (requires file_path, line, column; optional `direction` parameter — incoming, outgoing, or both, default both; returns `CallHierarchySummary` with items, incoming, outgoing, errors, truncated). typeHierarchy (requires file_path, line, column; optional `direction` parameter; returns `TypeHierarchySummary` with items, supertypes, subtypes, errors, truncated). Previews return `WorkspaceEditPreview` (unified diff patches + hashes + `patch_omitted` flag); previews are read-only — actual mutation stays in the mutating `apply_patch` tool. `lsp` tool is always `ToolCategory::ReadOnly`. Command-only source actions are rejected because command execution is disabled. `execute_structured` checks both `/results/restore_error` and `/results/overlay/restore_error` for success detection. |

### Security Operations

| Tool | File | Description |
|------|------|-------------|
| **security** | `security.rs` | Analyze code for security vulnerabilities. Checks for SQL injection, XSS, command injection, path traversal, and other common security issues. |

**`securityContext`**: Read-only security-review packet. Returns deterministic risk markers plus bounded LSP context. Not a vulnerability scanner. Never writes files.

**`callHierarchy` / `typeHierarchy`**: Read-only, shallow, bounded hierarchy summaries. Require `file_path`, `line`, and `column`.

`securityContext` is a security-review context packet operation. It reuses the same LSP infrastructure as `semanticContext` but adds deterministic risk marker scanning over the source excerpt. Risk markers use pattern matching against known security-sensitive code patterns (process execution, unsafe blocks, filesystem access, network boundaries, etc.). The scanner is bounded, deterministic, and does not execute code or run external tools. Output includes filtered symbols/diagnostics prioritized by security relevance, optional call hierarchy, optional bounded call expansion, and optional overlay diagnostics for proposed patches. Risk marker scanning, pattern tables, and security-relevant filtering helpers live in `src/tool/lsp_security.rs`. Diagnostics and symbols are filtered for security relevance before capping; truncation flags are precise (reflect filtered counts, not raw counts). Nonfatal LSP subrequest failures are surfaced in the `notes` array. Presets via `security_preset` (`rust_server`, `rust_cli`, `web_backend`, `dependency_review`, `unsafe_review`) tune default risk categories, excerpt radius, marker count, and call-hierarchy inclusion; explicit user inputs override preset defaults. Call expansion (`call_depth` 0/1/2) performs BFS-based recursive call hierarchy traversal with dedup, capped by `max_call_nodes` (default 32, max 64); no preset enables expansion by default.

#### Security review workflow

The `security` tool is also used by the `security-review` agent as part of a structured workflow (`src/security/workflow/` — split into submodules: `mod.rs`, `types.rs`, `diff.rs`, `preflight.rs`, `evidence.rs`, `context.rs`, `report.rs`, `enrichment.rs`, `receipt.rs`). The workflow discovers changed hunks via git diff, selects presets via path heuristics, runs deterministic filename-hint preflight checks (`secret_filename_hint_scan`, `unsafe_filename_hint_scan`), and synthesizes prompts from risk markers and evidence. Risk markers always produce review prompts, never confirmed findings. Planned target prompts include `source: changed_hunk` evidence; risk-marker prompts include `source: securityContext.risk_marker` evidence, making the two sources distinguishable. The result panel renders hunk context (added/removed/context line styling) for findings/prompts matched to hunks by file_path + line in range; the `HunkBacked` filter shows only hunk-backed items.

The async orchestrator `run_security_review_workflow(root, base, options)` runs the full pipeline (discover → preflight → evidence-based synthesis → assemble) without executing `securityContext` LSP requests. `SecurityReviewWorkflowOptions` controls stages and output caps. Content preflight uses `root.join(p)` for repo-root-relative reads, so it works correctly from any working directory. The escalation policy (`choose_security_context_escalation`) maps risk signals to bounded `SecurityContextEscalationLevel` values (None/Basic/CallDepth1/CallDepth2) for selective LSP call expansion. `plan_security_context_escalations()` returns a `SecurityContextEscalationPlan` DTO as a policy recommendation — it does not execute LSP requests.

An optional LSP enrichment pass (`--enrich`) executes bounded, read-only `securityContext` requests for escalated targets via the `SecurityContextExecutor` trait, then reruns synthesis with enriched CallPath/Diagnostic/TruncationNotice evidence. The `LspSecurityContextExecutor` adapter (in `src/security/lsp_executor.rs`) wraps `Arc<LspTool>` and implements `SecurityContextExecutor`, validating requests via `validate_security_context_request()` before delegation. The `SecurityContextExecutorProvider` trait (`fn security_context_executor(&self) -> Option<Arc<dyn SecurityContextExecutor>>`) enables executor injection at the command level. `run_security_review_command_with_executor()` accepts `Option<&dyn SecurityContextExecutor>`; `run_security_review_command()` delegates to it with `None`. In local mode the TUI creates a shared `LspTool` at startup (`App.lsp_tool`) and passes a `LspSecurityContextExecutor` to the command handler for `--enrich`. In socket/remote mode `lsp_tool` is `None` and `--enrich` falls back to deterministic stage-1 with a `note_lsp_enrichment_unavailable` note. Additional note helpers (`note_lsp_enrichment_no_eligible_targets`, `note_lsp_enrichment_executed`) report enrichment status. Enrichment is opt-in, fail-soft, and never mutates files.

The `/security-review` TUI command exposes the workflow. The command handler is testable via `parse_security_review_args()` and `run_security_review_command()` in `src/security/workflow/report.rs`. Flags: `--changed` (shorthand for `--base HEAD`), `--base <ref>`, `--json`, `--prompts-only`, `--findings-only`, `--no-content`, `--no-filename`, `--max-findings N`, `--max-prompts N`, `--enrich`, `--max-enriched-targets N`, `--lsp-timeout-ms N`, `--panel` (auto-open result panel on completion). The handler dispatches asynchronously via `TuiCommand::SecurityReviewRun { id, root, args, lsp_tool }` (consumed in `run_event_loop`'s `cmd_rx` arm by `handle_security_review_run` in `src/tui/mod.rs`); the TUI render thread is never blocked. A reentrancy guard `App.security_review_running: Option<SecurityReviewTaskState>` (holding `{ id, abort_handle }`, defined in `src/security/workflow/receipt.rs:301`) rejects a second concurrent `/security-review` with a warning toast. The new `run_security_review_background(root: PathBuf, args: SecurityReviewCommandArgs, lsp_tool: Option<Arc<LspTool>>) -> Result<SecurityReviewReceipt, String>` helper in `src/security/workflow/report.rs` owns its inputs and constructs the `LspSecurityContextExecutor` internally, so the dispatcher can spawn it. On success the full report is pushed to the message timeline as an Assistant message with a `[Security Review]` label plus a brief success toast AND the structured `SecurityReviewReceipt` is stored on `App.latest_security_review`; on failure an error toast is shown. `/security-review-show` reopens the latest receipt via `Dialog::SecurityReview` without rerunning the review. `/security-review-cancel` aborts an in-flight review via `AbortHandle::abort()`; stale completions (id mismatch) are silently dropped by the completion handler. The result panel's `Enter` key opens a read-only source preview dialog for the finding's file (root-scoped via `resolve_security_review_item_path` in `receipt.rs`; falls back to clipboard if the file cannot be opened). Receipt persistence is in-memory only. The review is read-only by design — no file mutations.

### Meta Operations

| Tool | File | Description |
|------|------|-------------|
| **batch** | `batch.rs` | Execute up to 25 tool calls in parallel. Each call limited to 100KB input, total output limited to 500KB. |
| **tool_search** | `tool_search.rs` | On-demand tool discovery. Searches catalog by name/description. Registered with catalog (not as a regular tool). |
| **invalid** | `invalid.rs` | Catch-all for malformed tool calls. Returns tool name and error message. |

### Deterministic Tools (eggsact)

In-process deterministic correctness utilities backed by the `eggsact` crate.
See [deterministic_tools.md](deterministic_tools.md) for the full catalog,
configuration, and integration details. Implemented via the generic
`EggsactTool` wrapper in `src/tool/deterministic.rs`.
All use `ToolCategory::ReadOnly` and are registered best-effort — if `EggsactRuntime::new()` fails, the tools are silently skipped.

**Always-visible (8 tools):**

| Tool | Eggsact Name | Description |
|------|-------------|-------------|
| **text_equal** | `text_equal` | Compare two strings for equality under various modes (raw, normalized, casefolded, trimmed). |
| **text_diff_explain** | `text_diff_explain` | Explain why two strings differ with Unicode-aware span analysis. |
| **text_replace_check** | `text_replace_check` | Check whether a text replacement would apply cleanly before editing. |
| **validate_json** | `validate_json` | Validate JSON syntax and report precise parse errors. |
| **validate_toml** | `validate_toml` | Validate TOML files and report parse errors with line/column. |
| **command_preflight** | `command_preflight` | Analyze a shell command before execution: parse argv, detect features, find risk patterns. |
| **path_normalize** | `path_normalize` | Normalize a filesystem path: collapse dot segments, resolve components. |
| **text_security_inspect** | `text_security_inspect` | Security-oriented text hygiene pass: detect hidden chars, confusables, prompt injection. |

**Deferred / contextual (5 tools, discoverable via `tool_search`):**

| Tool | Eggsact Name | Description |
|------|-------------|-------------|
| **text_inspect** | `text_inspect` | Inspect a string for hidden characters, Unicode confusables, mixed scripts. |
| **config_preflight** | `config_preflight` | Validate generated config text. Auto-detects format and runs appropriate validator. |
| **identifier_inspect** | `identifier_inspect` | Inspect identifiers for validity and collisions. |
| **structured_data_compare** | `structured_data_compare` | Compare structured config/data output (JSON). |
| **text_fingerprint** | `text_fingerprint` | Compute a deterministic SHA-256 fingerprint of text. |

The `build_eggsact_tools(runtime)` function returns `(Vec<EggsactTool>, Vec<EggsactTool>)` — always-visible and deferred sets.
All tools tag provenance with `backend = "native"`, `implementation = "eggsact/<tool_name>"`, `trust = LocalTrusted`.
The adapter module is at `src/eggsact/adapter.rs`; config schema is `[deterministic_tools]` in `crates/codegg-config/src/schema.rs`.

### Harness-Side Preflight Integration

Mutating tools can optionally run preflight checks before executing. The preflight
service (`src/preflight/service.rs`) wraps the same eggsact runtime used by the
deterministic tools but operates **harness-internal only** — preflight calls never
appear as model-facing tool calls.

**Module**: `src/preflight/` (types in `mod.rs`, implementation in `service.rs`)

**Config**: `[preflight]` in opencode.json (schema: `PreflightConfig` in `crates/codegg-config/src/schema.rs`)

**Key types**:
- `PreflightService` — wraps `EggsactRuntime` with a `PreflightPolicy`
- `PreflightPolicy` — controls mode (`off`/`observe`/`warn`/`block_on_definite`), per-category toggles (patch, config, shell, unicode), and output options
- `PreflightDecision` — `Allow`/`Warn`/`Block` with findings
- `PreflightFinding` — severity-classified result with machine code, message, location, source tool

**Integration points**: `check_text_replace` (edit/replace), `check_json_valid`/`check_toml_valid`/`check_config` (config writes), `check_command` (bash), `check_text_security` (unicode safety). Tool integration is opt-in — each tool calls the relevant check method before executing.

**Anti-recursion**: Preflight uses the eggsact runtime directly (not through `ToolRegistry`), so it cannot trigger tool execution cycles. The service is constructed with `audience = "harness"` to distinguish it from model-facing tool calls.

**Default policy**: enabled, mode `warn`, all categories on, findings logged and surfaced in tool output.

### Test Matrix (Phase 7)

The deterministic tools and preflight system are covered by a comprehensive test matrix:

- **Eggsact adapter**: Unit tests for `format_response`, `to_structured_result`, and `EggsactConfig` defaults. Integration tests for all 8 always-visible tools, 5 deferred tools, provenance, audience filtering, and truncation.
- **Harness preflight**: Integration tests for `check_text_replace`, `check_json_valid`, `check_toml_valid`, `check_command`, `check_text_security` with real eggsact calls. Policy mode tests for off/observe/warn/block_on_definite.
- **Tool registry**: Tests verifying deferred tools are not in default definitions but discoverable via tool_search, descriptions imply no mutation, and disabled backend hides wrappers.

## NOT Registered (exists but excluded from default registry)

**multiedit** (`src/tool/multiedit.rs`):
- Module exists and is registered via `pub mod multiedit` in `mod.rs`
- NOT included in `ToolRegistry::with_defaults()`
- Applies multiple edit operations to a single file sequentially
- Uses same path validation as other file tools

To register multiedit, add to `with_defaults()`:
```rust
registry.register(crate::tool::multiedit::MultiEditTool::default());
```

## ToolRegistry

Manages registration and lookup of tools at `src/tool/mod.rs:163-167`:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: catalog::ToolCatalog,
    tool_backends: ToolBackendConfig,
    integrated_config: IntegratedToolRuntimeConfig,
}
```

### Methods

| Method | Description |
|--------|-------------|
| `new()` | Create empty registry |
| `with_options(ToolRegistryOptions)` | Authoritative registration sequence; the other constructors are thin wrappers |
| `with_defaults()` | Create registry with all built-in tools, all-native backend defaults |
| `with_config(&Config)` | Resolves `ToolBackendConfig::from_config` + `IntegratedToolRuntimeConfig::resolve_integrated_config` and passes both through `with_options`. Used by CLI exec/chat and subagents. |
| `with_session_config_defaults(&Config, todo_state, policy, pool, session_id)` | **Production session constructor.** Resolves both `ToolBackendConfig::from_config(&Config)` and `IntegratedToolRuntimeConfig` and threads them through `with_options`, so resolved `[tool_backends]`, `[deterministic_tools]`, `[preflight]`, and `[search]` configs are preserved. |
| `with_session_defaults(todo_state, policy, pool, session_id)` | Session registry with **all-native backend defaults** — drops any loaded `[tool_backends]` and integrated config. Kept for tests and non-config-aware callers; the doc comment warns against using it in production paths. |
| `register(&mut self, tool: impl Tool + 'static)` | Register a tool (takes owned value via `impl Tool + 'static`) |
| `get(&self, name: &str) -> Option<&dyn Tool>` | Get tool by name (includes hidden stubs) |
| `list(&self) -> Vec<&dyn Tool>` | List all tools (includes hidden stubs) |
| `filter_out(&mut self, denied_tools: &[String])` | Remove denied tools from registry |
| `definitions(&self) -> Vec<ToolDefinition>` | Get tool definitions for LLM — filters via `Tool::expose_in_definitions()` so `DisabledTool` stubs are hidden |
| `catalog(&self) -> &ToolCatalog` | Access the tool catalog |
| `set_search_mode(&mut self, mode)` | Set tool catalog search mode |
| `register_deferred_names(&mut self, names)` | Register names of tools that load on-demand |
| `set_search_tool_available_tools(&mut self, available)` | Inject available tool names into `tool_search` |
| `execute_capture(name, input, ctx) -> StructuredToolResult` | Central execution path used by `AgentLoop::execute_tool_calls` for native tools. Returns structured provenance; the model-facing `structured.output` matches the legacy `execute()` string. |
| `tool_backends()` | Resolved `ToolBackendConfig` captured at construction |
| `integrated_config()` | Resolved `IntegratedToolRuntimeConfig` (evidence/deterministic/preflight) captured at construction |
| `backend_report(mcp_server_names)` | Runtime-aware status report for `/tool-backends` (Active / FallbackToNative / Disabled / ConfiguredButUnavailable) |

### ToolRegistryOptions (Phase 2)

Centralizes all knobs that influence registration:

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
}
```

The last three fields are resolved by `integrated_config::resolve_integrated_config(&Config)` and passed through from `with_config()`, `with_session_config_defaults()`, and `build_session_tool_registry()`. `with_defaults()` and `with_session_defaults()` pass `None` for these (tests only).

Both `with_defaults()` and `with_session_*_defaults(...)` build a
`ToolRegistryOptions` and delegate to `with_options()`. LSP service
construction is now injectable instead of hardcoded in two places.

### Integrated Tool Runtime Config (Phase 6)

`src/tool/integrated_config.rs` resolves evidence, deterministic, and
preflight runtime configs from the loaded `Config` in one pass:

```rust
pub struct IntegratedToolRuntimeConfig {
    pub evidence: EvidenceBackendRuntimeConfig,     // from [search]
    pub deterministic: DeterministicToolsRuntimeConfig, // from [deterministic_tools]
    pub preflight: PreflightRuntimeConfig,           // from [preflight]
}
```

Entry point: `resolve_integrated_config(&Config) -> IntegratedToolRuntimeConfig`.

- **Evidence**: `search_backend`, `expose_raw_mcp_tools`, `fallback_to_builtin`
- **Deterministic**: `enabled`, `backend`, `profile` (validated against `KNOWN_EGGSACT_PROFILES`: `codegg_core`, `codegg_core_min`, `default`, `full`), `model_audience`, `harness_audience`, `expose_expert_tools`, `max_output_chars`
- **Preflight**: `enabled`, `mode` (off/observe/warn/block_on_definite), `log_findings`, `model_visible_findings`

The resolved config is stashed on `ToolRegistry.integrated_config` and
accessible via `registry.integrated_config()`. It is consumed by:
- `with_options()` — passes `deterministic_config` to `EggsactConfig` instead of hardcoded defaults; respects `enabled` and `backend != "disabled"`
- `build_report()` — adds deterministic/preflight/evidence rows to `/tool-backends`
- Subagent construction (`worker.rs:698`) — now uses `with_config(&config)` which resolves integrated config, instead of `with_defaults()` which dropped it

### Native tool execution path

Native tool wrappers (e.g. `lsp`, `security`, `git`, `review`) call
into the corresponding workspace crate (`egglsp`, `eggsentry`, `egggit`)
for actual work. Crate local config types are converted from Codegg's
`crate::config::schema::*` types at the bridge site. See
`architecture/native_crates.md` for the full boundary, public APIs,
and provenance model.

The central execution path for native tools in
`AgentLoop::execute_tool_calls` (`src/agent/loop.rs`) is
`ToolRegistry::execute_capture(name, input, ctx)`. It calls
`Tool::execute_structured()` internally, populates a fallback
`ToolProvenance::legacy(...)` for tools that do not override it, and
records provenance via `tracing::debug!` (backend, implementation,
elapsed_ms). The returned `StructuredToolResult` is collapsed to
`structured.output` for the model — the model-facing string is
identical to the legacy `execute()` path. MCP tools
(`mcp__server__tool`) continue to dispatch through
`McpService::call_tool` and are not funnelled through
`execute_capture`.

The `ToolExecutionContext` passed to `execute_capture` is built by the
small helper `AgentLoop::build_tool_execution_context(tc, timeout_ms)`
(`src/agent/loop.rs`). It fills in `session_id`, `cwd`, `timeout_ms`,
and the resolved `ToolBackendKind`. Backend resolution is delegated
to `AgentLoop::resolve_native_backend(name)`: most tools resolve to
`Native`, while `websearch` / `webfetch` resolve to `Mcp` when
`[search].backend = eggsearch` and to `BuiltinLegacy` for the
`builtin` or `disabled` configurations. After the call returns, the
dispatcher emits a `tracing::debug!` line summarising the
`ToolProvenance` (backend, implementation, elapsed_ms, trust) so the
structured metadata stays internal and never reaches the model.

Regression coverage:

- `tests/tool_structured_execution.rs` — locks down the
  `ToolRegistry::execute_capture` contract (provenance shape,
  disabled/MCP-fallback semantics, definition visibility).
- `tests/agent_loop_harness.rs::test_live_dispatcher_uses_execute_capture`
  — proves the live agent-loop dispatcher routes native calls through
  `execute_capture`. The mock tool overrides `execute_structured`
  to record the call; if the dispatcher ever bypassed the structured
  path the recording would not fire and the test would fail.
- `tests/agent_loop_harness.rs::test_live_dispatcher_model_output_shape_is_plain_string`
  — locks down the model-facing `Message::Tool` content: it must
  match the raw tool output string and contain no provenance
  envelope (`provenance`, `backend`, `implementation`, `trust`,
  `elapsed_ms`).

### `expose_in_definitions` filtering

`Tool::expose_in_definitions()` (default `true`) is the model-facing
predicate. `DisabledTool` overrides it to `false`, so
`ToolRegistry::definitions()` and `AgentLoop::build_tool_definitions()`
both filter the tool out of the model-visible catalog. The stub
remains registered and callable by name so:

- `/tool-backends` and `ToolRegistry::backend_report(...)` can
  introspect the disabled/MCP-stub state.
- Tests can call the stub to assert the error message.
- The disabled reason remains in the registry for diagnostics.

Because both `definitions()` and `build_tool_definitions()` apply the
same predicate, the model's view of the tool surface and
`tool_search`'s view stay in lockstep: disabled/MCP-stub tools are
never advertised.

## ToolCatalog

Provides metadata management and search at `src/tool/catalog.rs:32-40`:

```rust
pub struct ToolCatalog {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
}
```

### ToolMetadata

```rust
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub defer_load: bool,
}
```

### Catalog Methods

| Method | Description |
|--------|-------------|
| `register(&mut self, tool: &dyn Tool)` | Register tool metadata (takes reference, not owned) |
| `search(&self, query: &str) -> Vec<&ToolMetadata>` | Search by name or description (case-insensitive) |
| `get(&self, name: &str) -> Option<&ToolMetadata>` | Get metadata by name |
| `list(&self) -> Vec<&ToolMetadata>` | List all metadata |
| `deferred_tools(&self) -> Vec<&ToolMetadata>` | List tools marked for deferred loading |
| `is_deferred(&self, name: &str) -> bool` | Check if tool is deferred |

## Tool Execution Flow

```
AgentLoop
├── Provider sends ToolCall event
├── ToolRegistry::get(tool_name)
│   └── tool.execute(input)
│       ├── Parameter extraction
│       ├── Path validation (for file tools)
│       ├── Permission checking
│       └── Execute tool logic
└── Return Result<String, ToolError>
```

### Execution Details by Tool Type

**File Tools** (read, write, edit, glob, grep, list, diff, replace, apply_patch):
1. Extract path from input JSON
2. Call `validate_path()` or `canonicalize_path()` from `util.rs`
3. Check symlinks with `check_path_for_symlinks()`
4. Perform operation in `tokio::task::spawn_blocking()`
5. Publish `AppEvent::FileChanged` for mutations

**Shell Tools** (bash, terminal):
1. Extract command from input
2. Check against `BLOCKED_PATTERN` regex
3. Check against `blocked_commands` HashSet
4. Validate allowlist if configured
5. Execute via `tokio::process::Command`
6. Apply output truncation (2000 lines, 50KB default)

**Web Tools** (webfetch, websearch, codesearch, image):
1. `websearch`/`webfetch` dispatch to the configured backend via
   `search_backend`. With the default `eggsearch` backend, SSRF
   protection is delegated to the eggsearch subprocess; with the
   `builtin` backend, `tool::webfetch::execute_builtin` runs the
   steps below.
2. `image` (and the legacy `builtin` webfetch) parse the URL,
   call `validate_host_ip()` for SSRF protection, then
   `revalidate_dns()` to verify DNS, then make the HTTP request
   with appropriate headers and process the response (markdown
   for HTML, base64 for images).

**Subagent Tools** (task):
1. Create task in TaskStore
2. Send to SubAgentSpawner
3. Return task_id for later retrieval via `action=get`

## Path Validation

All file operations use utility functions from `src/tool/util.rs`:

### validate_path (for restricted tools)

```rust
pub fn validate_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    let canonical = canonicalize_path_internal(path)?;
    let root_canonical = allowed_root.canonicalize()?;
    if !canonical.starts_with(&root_canonical) {
        return Err(ToolError::Permission(format!(
            "path '{}' is outside allowed directory",
            path.display()
        )));
    }
    Ok(canonical)
}
```

### canonicalize_path (for unrestricted tools)

```rust
pub fn canonicalize_path(path: &Path) -> Result<PathBuf, ToolError> {
    check_path_for_symlinks(path)?;
    canonicalize_path_internal(path)
}
```

### check_path_for_symlinks

```rust
pub fn check_path_for_symlinks(path: &Path) -> Result<(), ToolError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if current.symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false) {
            return Err(ToolError::Permission(format!(
                "symlink not allowed in path: {}",
                current.display()
            )));
        }
    }
    Ok(())
}
```

### Key Validation Points

- **Symlinks rejected**: Paths containing symlinks are rejected
- **Allowed root enforcement**: File tools restrict operations to within `allowed_root`
- **spawn_blocking for I/O**: All file I/O happens in `tokio::task::spawn_blocking()` to avoid blocking the async runtime
- **Absolute path handling**: Relative paths are joined with allowed_root before validation

## ToolError

Defined in `src/error.rs`, used by all tools:

```rust
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("tool execution failed: {0}")]
    Execution(String),
    #[error("tool timeout: {0}")]
    Timeout(String),
    #[error("permission denied: {0}")]
    Permission(String),
    #[error("tool formatting failed: {0}")]
    Format(String),
    #[error("tool disabled: {0}")]
    Disabled(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("network error: {0}")]
    Network(String),
}

impl ToolError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_))
    }
}
```

## Security Considerations

1. **Tool path validation**: All file paths validated before access
2. **Symlink protection**: `check_path_for_symlinks()` rejects paths containing symlinks
3. **Permission enforcement**: Tools check permissions before execution
4. **BashTool blocked patterns**: Regex-based detection of 40+ dangerous command patterns
5. **BashTool blocked commands**: HashSet of full commands that are blocked (rm -rf /, mkfs, etc.)
6. **SSRF protection**: WebFetch validates URLs against internal IP ranges
7. **Subprocess PATH**: External processes use `std::env::var_os("PATH")` (not hardcoded)
8. **Environment variable filtering**: TerminalTool filters dangerous env vars (LD_PRELOAD, DYLD_*)
9. **Allowlist support**: BashTool and TerminalTool support command allowlists

## Tool Backend Diagnostics (Phase 10)

`/tool-backends` (aliases `/tools`, `/backends`) surfaces the native vs
MCP wiring of every model-facing tool. The handler builds a
synchronous report from the resolved `ToolBackendConfig` plus
`IntegratedToolRuntimeConfig` (evidence/deterministic/preflight) and any
pre-installed context (e.g. eggsearch availability from
`search_backend::state`) and renders it as a toast. The report shape:

```
Tool              Backend   Implementation    Status       Raw MCP exposed
websearch         MCP       eggsearch          ready        no
webfetch          MCP       eggsearch          ready        no
repo_search       MCP       eggsearch          ready        no
repo_fetch        MCP       eggsearch          ready        no
repo_map          MCP       eggsearch          ready        no
security_search   MCP       eggsearch          ready        no
research_search   MCP       eggsearch          ready        no
batch_fetch       MCP       eggsearch          ready        no
evidence_bundle   MCP       eggsearch          ready        no
lsp               Native    egglsp             ready        n/a
security          Native    eggsentry          ready        n/a
git               Native    codegg/egggit      ready        n/a
deterministic     native    eggsact/codegg_core enabled      n/a
preflight         native    eggsact/codegg_core warn         n/a
evidence          MCP       eggsearch          ready        no
```

The `deterministic` row shows the eggsact profile and enabled/disabled
state. The `preflight` row shows the eggsact profile and the policy
mode (off/observe/warn/block_on_definite). The `evidence` row shows
the search backend connection status.

Status values are: `ready`, `disabled`, `unavailable`, `error(<msg>)`.
Warnings are appended when a backend is configured-but-unavailable or
when raw MCP tools are hidden because a native wrapper is active.

See `architecture/native_crates.md` for the underlying contract
(`ToolBackendKind`, `ToolProvenance`, `McpExposurePolicy`).

## File Structure Summary

```
src/tool/
├── mod.rs          # Tool trait, ToolRegistry, with_options() / with_defaults() / with_session_defaults()
├── backend.rs      # ToolBackendKind, ToolProvenance, ToolExecutionContext, StructuredToolResult,
│                   # ToolBackendConfig, build_report() for /tool-backends
├── backend_config.rs  # ToolBackendConfig::from_config() — schema → runtime conversion
├── integrated_config.rs  # IntegratedToolRuntimeConfig: resolve_integrated_config() for evidence/deterministic/preflight
├── factory.rs      # build_session_tool_registry() — canonical production constructor for daemon sessions
├── catalog.rs      # ToolCatalog for metadata and search
├── util.rs         # Path validation helpers
├── bash.rs         # Shell command execution
├── read.rs         # File reading with image/PDF base64 support
├── write.rs        # File writing with auto-formatting
├── edit.rs         # 8-strategy edit matching
├── glob.rs         # Glob pattern file finding
├── grep.rs         # Regex content search
├── list.rs         # Directory tree listing
├── diff.rs         # Unified diff generation
├── replace.rs      # Regex find/replace
├── apply_patch.rs  # Unified diff patch application
├── patch_util.rs   # Shared patch utility functions for apply_patch and LSP preview
├── task.rs         # Subagent task spawning
├── todo.rs         # Todo list management
├── webfetch.rs     # URL content fetching (dispatches to search_backend)
├── websearch.rs    # Web search (dispatches to search_backend)
├── repo_search.rs  # Repository search (dispatches to search_backend)
├── repo_fetch.rs   # Repository file fetch (dispatches to search_backend)
├── repo_map.rs     # Repository directory map (dispatches to search_backend)
├── security_search.rs  # Security advisory search (dispatches to search_backend)
├── research_search.rs  # Academic/research search (dispatches to search_backend)
├── batch_fetch.rs  # Batch URL fetch (dispatches to search_backend)
├── evidence_bundle.rs  # Evidence bundle builder (dispatches to search_backend)
├── codesearch.rs   # Code search via Exa
├── question.rs     # User question asking
├── skill.rs        # Skill loading
├── review.rs       # LLM-based code review (uses egggit::diff_summary)
├── batch.rs        # Parallel tool execution
├── terminal.rs     # Terminal command execution
├── test.rs         # Supervised test runner (wraps test_runner module, includes previous-failures index)
├── git.rs          # Git command execution (low-level wrapper)
├── commit.rs       # LLM-generated commit messages
├── plan.rs         # plan_enter and plan_exit tools
├── invalid.rs      # Malformed call handler
├── multiedit.rs    # Multi-edit tool (NOT registered)
├── image.rs        # DALL-E image generation
├── tool_search.rs  # On-demand tool discovery
├── lsp.rs          # LSP client tools (wraps egglsp::LspService)
├── security.rs     # Security scanning (wraps eggsentry)
├── teams.rs        # Team operation tools
├── formatter.rs    # Auto-formatting support
└── ...
```

## Tool Contracts and the Canonical Broker

All production tool calls are routed through the **ToolBroker**
(`src/tool/broker.rs`), which enforces a 10-step policy pipeline:

1. **Lookup** — resolve `ToolContract` from pre-built catalog
2. **Caller policy** — check `ToolCallerPolicy` against `ToolCaller`
3. **Input validation** — schema and size bounds
4. **Authority/permission** — delegation to permission system
5. **Deadline/cancellation** — effective timeout resolution
6. **Route selection** — inline native or scheduler-owned (future)
7. **Execution** — `Tool::execute_structured` via registry
8. **Output validation** — size bounds and truncation
9. **Artifact registration** — large body handles
10. **Terminal result** — `ToolValue` with status and provenance

Each tool has a `ToolContract` describing its caller policy, effect
class, idempotency, retry/cache policies, and projection policy.
Legacy tools receive conservative defaults via
`ToolContract::legacy()`.

See [tool_broker.md](tool_broker.md) for the full contract and
pipeline documentation.

## See Also

- [tool_broker.md](tool_broker.md) - Canonical execution boundary for all production tool calls
- [agent.md](agent.md) - Uses ToolRegistry for tool execution
- [permission.md](permission.md) - Permission checking before execution
- [snapshot.md](snapshot.md) - File state capture before modifications
- [security.md](security.md) - SSRF and path validation
- [agent.md](agent.md) - SubAgentPool and TaskStore for task tool
