# Tool Module

## Progressive discovery and semantic read surfaces

`tool_search` uses a two-stage contract. Broad queries return bounded,
schema-free descriptors so selecting among large tools does not inflate the
context window. An exact selected name can request `detail: "schema"`; the
response is resolved from the live catalog after the current policy and
hidden-tool checks. Multiword queries are token-ranked with stable name
tie-breaks. An empty query lists only policy-allowed descriptors in bounded
`offset`/`limit` pages (default 10, maximum 50); no schemas are included until
one exact name is selected. The expanded schema is descriptive only; calls
still enter through `ToolBroker`.

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
- Built-in tool implementations (67 registration sites in `with_options()` —
  54 `register` + 13 `register_boxed` — resolving 40 unconditionally
  registered tool names plus conditional eggsact, evidence, memory,
  extension, todo, work-plan, context, and disabled-backend variants; 61
  distinct production tool names exist across `src/tool/` and
  `src/context/`)
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
│   │                   # plus Bash-owned policy glue (advisory risk patterns,
│   │                   # explicit blocked/allow lists, child worktree checks, kill
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
├── lsp_read.rs         # Hidden program-only LSP read adapter (ProgrammaticOnly)
├── lsp_preview_apply.rs# Direct-only checked LSP preview application
├── security.rs         # Security scanning (wraps eggsentry)
├── git_read.rs         # Hidden program-only Git read adapter + git_query facade
├── goal.rs             # goal_get / goal_update_progress / goal_request_completion
├── job.rs              # Background job inspection and control
├── ci_watch.rs         # GitHub Actions workflow-run watch
├── work_order.rs       # Durable future-session creation
├── work_plan.rs        # work_plan_get / work_plan_update_item
├── disclosure.rs       # disclosure_for(): Core / Deferred / Hidden / ProfileSpecific
├── authorization.rs    # CommandPreflightResult and the authority handoff types
├── process_admission.rs# M005 C005 shared-process admission policy
├── deterministic.rs    # EggsactTool wrapper, build_eggsact_tools()
└── ...
```

For commands issued inside an already-authorized workspace/action, ordinary
shell grammar and language-runner syntax do not create a second approval by
themselves. Explicit operator rules and concrete risk findings still feed the
existing authorization route. A requested sandbox is reported according to the
backend actually obtained; Landlock-only Linux runs do not restrict network or
Unix IPC. The human-owned `!` shell keeps its separate confirmation policy.

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

The regular-tool semaphore is local to one provider response. Its admission
deadline is separate from each call's execution timeout and does not stand in
for a scheduler permit or imply cross-agent capacity.

See `architecture/snapshot.md` for the `EditCheckpoint` storage
contract and `architecture/agent.md` for the `ToolBatchExecutor`
ownership.

## Tool Trait

Defined in `src/tool/mod.rs:163-256`:

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

Defined in `src/tool/mod.rs:139-150`, the category drives permission
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
| Discoverable? | `ToolCatalog` + `tool_search` | Policy-allowed tools with canonical name, category, risk, disclosure metadata. The catalog is live for late registration, results are deterministic, searches return at most 10 by default, and empty-query browsing is paged at up to 50. |
| Callable? | `ToolBroker` + permission + contracts | Disclosure never widens authority; denied/hidden tools stay non-callable. |

Operator views (`/tool-backends`, diagnostics) report registered
capability, not just the currently advertised prompt slice. Deferred
tools are not hidden from operators.

### Always-Registered Tools (40 names)

`ToolRegistry::with_options()` (`src/tool/mod.rs:587`) contains **67**
registration call sites — 54 `registry.register(..)` and 13
`registry.register_boxed(..)`. 37 of those sites are unconditional; the
remaining 30 sit behind conditionals (see the conditional sections below).

Counting *distinct names* rather than sites: the 37 unconditional sites
each register a different tool, and three more names are registered by
mutually exclusive variant sites that always resolve to exactly one
variant — `lsp` (native / disabled stub / MCP-with-fallback /
MCP-no-fallback, plus the constrained-profile disabled stub), `lsp_read`
(constrained stub vs. native adapter), and `security` (native / disabled
stub / MCP-with-fallback / MCP-no-fallback). That is **40 distinct names
registered for every registry configuration**. `todowrite` is *not* in
that set — see the todo section below.

Two adjacent counts are load-bearing and easy to conflate with the above:

- **75 distinct production tool names** — the `name()` values of the 67
  production `impl Tool for` blocks under `src/` (5 of them resolve the name
  at runtime), **plus** the 13 `eggsact` deterministic validator names, which
  resolve at runtime from `self.codegg_name` and are therefore counted at
  their `EggsactTool::new()` call sites in `src/tool/deterministic.rs`
  (62 literal + 13 runtime-named). The scan covers the **whole `src/` tree**,
  not only `src/tool/` and `src/context/`: a production `Tool` impl may live
  with the subsystem that owns the tool's behaviour, and `python_script`
  (`src/python_script/tool.rs`) is one — it is registered unconditionally by
  `with_options()` yet lives outside the registry directory, so a scan scoped
  to `src/tool/` + `src/context/` undercounts the production surface by one.
  This set is a superset of the registered names: `lsp_preview_apply`,
  `goal_get`, `goal_request_completion`, `goal_update_progress`, and
  `work_order` are registered later by `build_session_tool_registry()`
  (`src/tool/factory.rs`), and `batch` (`src/tool/batch.rs`) has a
  production `Tool` impl but no production registration at all.
- **83 tool source files** — `src/tool/**.rs`, including `src/tool/bash/`
  and excluding the `src/tool/bash/tests/` fixture.

`grep -c '\.register(' src/tool/mod.rs` (= 55) is not a tool count
either: it is the 54 `registry.register(..)` sites plus the
`catalog.register` call inside `ToolRegistry::register`. The two
deterministic-tool loops at `src/tool/mod.rs:1274`/`:1277` each register
many tools.

Disclosure is resolved by `disclosure_for()` in `src/tool/disclosure.rs:61`
(`Core` = ordinary immediate, `Deferred` = discoverable, `Hidden` = never
advertised):

| Tool | Disclosure | File | Description |
|------|------------|------|-------------|
| **bash** | Core | `bash.rs` + `bash/{policy,process,output}.rs` | Shell commands with advisory risk classification, explicit text filters, allowlist, and platform sandbox when available. Raw shell uses `sh -c` on Unix and the configured Windows PowerShell, `cmd.exe`, Git Bash, or WSL on Windows; structured argv bypasses shell parsing. Windows shell calls receive a cleaned per-run TEMP/TMP directory, but shell selection does not enable filesystem containment. 120s timeout. Facade sequences policy → execution → result. |
| **verify** | Core | `verify.rs` | Bounded Cargo `check`/`build`/`lint`/`typecheck`/`format_check`/`auto` facade delegating through `BashTool`. Takes no command/package/path field. |
| **read** | Core | `read.rs` | Read file contents with line numbers. Images/PDFs as base64. |
| **write** | Core | `write.rs` | Create or overwrite files with auto-formatting. |
| **edit** | Core | `edit.rs` | Surgical search-and-replace with 8 matching strategies. |
| **glob** | Core | `glob.rs` | Find files matching patterns relative to the validated search directory (gitignore-compliant); results are canonical paths. |
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
| **tool_search** | Core | `tool_search.rs` | On-demand discovery via multiword catalog ranking. Returns compact authorized descriptors, capped at 10 by default; empty queries browse authorized results in bounded pages. Exact names can request one schema. |
| **job** | Core | `job.rs` | Inspect and control the session's own background jobs (`status`/`wait`/`cancel`/`output`). Category: `ShellExec`. |
| **ci_watch** | Core | `ci_watch.rs` | Watch one GitHub Actions workflow run to completion (`create`/`status`/`cancel`/`list`). Category: `ShellExec`. |

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

### Conditional: Todo Tools (0-2 tools)

`todowrite` and `todoread` are both policy-gated via `TaskStatePolicy`
(`src/tool/mod.rs:864-896`):

| Tool | Condition | Description |
|------|-----------|-------------|
| **todowrite** | `todo_state` or `todo_policy` is `None` | Canonical `TodoWriteTool` with default in-memory state and the explicit-todo policy (no session persistence). The legacy `TodoTool` duplicate was removed in M001. |
| **todowrite** | `todo_state` and `todo_policy` both `Some`, and `policy.mode != Disabled && policy.allow_model_todo_write` | Policy-gated variant, with session persistence when `pool` + `session_id` are also present. |
| **todoread** | `todo_state` and `todo_policy` both `Some`, and `policy.allow_model_todo_read` | Read todo items. |

`todowrite` is therefore **not** unconditionally registered: with both
options present and the policy refusing model todo writes, no todo tool is
registered at all.

### Conditional: Memory Tools (2 tools)

Registered only when `ToolRegistryOptions.memory_store` is `Some`
(`src/tool/mod.rs:844-853`); scope derives from
`MemoryReadScope::for_project(project_identity)`:

| Tool | Disclosure | Description |
|------|------------|-------------|
| **memory_search** | Deferred | Bounded project-scoped memory search. |
| **memory_get** | Deferred | Bounded project-scoped memory read. |

### Conditional: Extension Tools (2 tools)

Registered only when `ToolRegistryOptions.extension_catalog` is `Some`
(`src/tool/mod.rs:854-861`):

| Tool | Disclosure | Description |
|------|------------|-------------|
| **extension_search** | Deferred | Search the host extension catalog. |
| **extension_install_request** | Deferred | Submit an extension install request (never installs directly). |

### Conditional: WorkPlan Tools (2 tools)

Registered only when both `pool` and `session_id` are `Some`
(`src/tool/mod.rs:902-909`):

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

Manages registration and lookup at `src/tool/mod.rs:342-361` (12 fields):

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
    decision_engine: Arc<dyn codegg_core::decision::DecisionEngine>,
    decision_backend: crate::tool_advisor::DecisionBackendResolution,
    advisor_mode: crate::tool_advisor::AdvisorMode,
    process_admission: Option<SharedProcessAdmission>, // M005 C005
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

Centralizes all knobs that influence registration (`src/tool/mod.rs:376`,
38 fields):

```rust
pub struct ToolRegistryOptions {
    pub tool_advisor: Option<ToolAdvisorConfig>,
    pub decision_engine: Option<DecisionEngineConfig>,
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
    pub publication: Option<Arc<GitIndexPublisher>>,
    pub submission: Option<Arc<JobSubmissionService>>,
    pub command_intent: Option<CommandIntentConfig>,
    pub windows_shell: Option<WindowsShellKind>,
    pub workspace_root: Option<PathBuf>,
    pub child_git_policy: Option<ChildGitPolicy>,
    pub git_read_only: Option<String>,
    pub asset_snapshot: Option<Arc<ProjectAssetSnapshot>>,
    pub asset_pin: Option<Arc<Mutex<RuntimeAssetPin>>>,
    pub notification_service: Option<Arc<ToolProgramNotificationService>>,
    pub job_notification_service: Option<Arc<JobNotificationService>>,
    pub ci_watch_service: Option<Arc<CiWatchService>>,
    pub search_runtime: Option<SearchRuntimeContext>,
    pub research_provider: Option<Arc<dyn Provider>>,
    pub research_model: Option<String>,
    pub sandbox_profile: Option<SandboxProfile>, // M005: None = WorkspaceWrite default
    pub memory_store: Option<Arc<dyn MemoryStore>>,
    pub project_identity: Option<String>,
    pub extension_catalog: Option<Arc<crate::plugin::marketplace::MarketplaceService>>,
    pub process_admission: Option<SharedProcessAdmission>, // M005 C005
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
`tests/security_permission/sandbox_policy_wiring.inc`.

**Workspace root vs. child worktree ceiling:** `BashTool` keeps two
independent roots. `workspace_root` is the ordinary session workspace —
the default `workdir` when the model omits one, the command-intent
classifier/planner context — and is populated for every registry that has
one, because `allowed_paths` is derived from it and an unqualified `workdir`
is otherwise rejected. `child_workspace_ceiling` is the separate lexical
"stay inside the isolated child-Git worktree" restriction enforced by
`policy::validate_child_workspace_command`, and it is armed only when
`child_git_policy` is configured. It must not be inferred from
`workspace_root`: it rejects *any* absolute path outside its root, so
applying it to ordinary sessions rejected every such command — most visibly
`2>/dev/null`, which tokenizes to the absolute path `/dev/null`.

**M002 accepted execution context:** the agent loop carries the accepted
`ExecutionPolicySnapshot`, permission receipt, and bounded command-preflight
summary through `ToolExecutionContext` and the broker invocation. Every broker
retry derives its attempt context from that same immutable value. Bash skips
its local preflight scan when this production context is present, avoiding a
second decision after approval. It verifies that the registered profile still
matches the accepted snapshot and refuses constrained execution when the
filesystem backend is unavailable. Raw Bash passes the context to
`ManagedProcessRequest`, which refuses a disabled sandbox when the accepted
snapshot requires filesystem containment. Typed Git mutation paths carry the
context into `GitMutationExecutor`; the typed Git policy remains responsible
for executable Git capabilities. Direct legacy tool calls without an accepted
context retain tool-local preflight behavior. This check does not establish
network or IPC confinement.

Command M004 route qualification is tracked in
`research/command-authorization-m004-route-coverage.md`. The active
command-intent routes dispatched from Bash remain explicit-FullHost-only;
classifier labels do not grant authority. The typed `test` and `python_script`
tools check the immutable accepted authorization before scheduler submission.
Test and Python now carry a valid accepted profile through their durable job
payloads to `ManagedProcessService`. Test uses managed streaming and refuses
ReadOnly before submission because test commands write build and report
artifacts. Python's mode policy can narrow WorkspaceWrite to read-only
execution, and ReadOnly refuses transform mode before submission. The managed
owner refuses a constrained job without a sandbox request. Corrective plan 008
tracks native effect and cross-platform qualification. Git filters/hooks and
remote operations remain behind Linux C002/C003 containment and transport
gates.

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

- **Evidence**: `search_backend`, `expose_raw_mcp_tools`; search backend is
  Eggsearch or disabled and does not switch providers on failure.
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
`src/tool/mod.rs:1701-1733` is the central execution path for native
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

Metadata management and search at `src/tool/catalog.rs:193-195`:

```rust
pub struct ToolCatalog {
    inner: Arc<RwLock<ToolCatalogState>>,
}

struct ToolCatalogState {
    tools: HashMap<String, ToolMetadata>,
    deferred_load: Vec<String>,
    search_mode: SearchMode,
    avg_doc_length: f64,
    doc_count: usize,
    idf_cache: HashMap<String, f64>,
}
```

`ToolCatalog::register()` takes `&dyn Tool` (not `Box<dyn Tool>`).
The catalog is cheaply cloneable: a clone shares the same
`Arc<RwLock<ToolCatalogState>>`.

Search supports keyword (case-insensitive substring) and BM25 ranking
modes. BM25 caches are recomputed on each registration when active.
`ToolCatalog::rank_descriptors()` (`src/tool/catalog.rs:220`) is the
shared scoring entry point used by both live discovery and the offline
advisor benchmark.

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

- **`validate_path(path, allowed_root)`**: Resolves the path against
  `allowed_root` when relative, symlink check + canonical root enforcement.
- **`validate_target_path(path, allowed_root)`**: Same rule for a target whose
  leaf and/or parents may not exist yet; the missing suffix is resolved
  lexically.
- **`canonicalize_path(path)`**: Symlink check + canonicalize. No root exists at
  this seam, so it is only for `unrestricted` callers.
- **`check_path_for_symlinks(path)`**: Walks components, rejects any symlink.

Key invariants:
- Symlinks rejected at every component
- Allowed root enforcement via canonical prefix check
- **A relative path is relative to `allowed_root`, never to the process
  working directory.** CodeGG executes turns inside a long-lived user-scoped
  daemon whose cwd is wherever it happened to be launched, so canonicalizing a
  raw relative path resolved it against that unrelated directory. A session
  rooted at one repository then failed to read a file that sat at the expected
  workspace path, while the failure hint — which *did* join the root — suggested
  the very file it reported as missing. `validate_path` and
  `validate_target_path` must stay in agreement: when only the latter joined the
  root, a `write` of a relative path landed correctly and the matching `read`
  could not read it back.
- The two root-bound helpers report the path **the caller sent**, not the
  derived absolute one, so a failure stays actionable.
- **Walk what you validated.** `glob` and `list` each computed a validated path
  and then discarded it, walking the raw request instead — `glob` starting from
  `.` and `list` starting from the *process* cwd. The containment filter then
  removed every match, so `glob` answered "No files matching …" for a workspace
  full of matches. A validation result that is computed, asserted, and thrown
  away authorizes one directory while the work happens in another. Use the
  canonical path returned by the helper as the thing you operate on.
- All file I/O in `tokio::task::spawn_blocking()`

### Tools that default a working directory

`TestTool` and `PythonScriptTool` take the registry's `workspace_root` as the
default `workdir`, the base a relative `workdir` resolves against, and (for
Python) the containment root. With no workspace authority they **refuse** an
omitted `workdir` rather than inventing one. Both previously had no workspace
root at all and fell back to `std::env::current_dir()`, so a session's tests and
scripts ran in the daemon's launch directory.

## Missing workspace root is typed, not defaulted

This is now a registry-wide contract rather than a per-tool exception.
`ToolRegistryOptions.workspace_root` is `Option<PathBuf>`, and `None` no longer
means "construct the tool anyway":

- `ToolRegistry::with_options` registers an `UnrootedTool`
  (`src/tool/disabled.rs`) for each workspace-scoped tool when the option is
  `None`. The stub keeps the tool's name and description, is hidden from the
  model's tool list, and returns `ToolError::MissingWorkspaceRoot { context }`
  if invoked. `ToolRegistry::register_boxed` exists because a stub and the real
  tool it replaces are unrelated concrete types.
- The tool `Default` constructors (`read`, `write`, `edit`, `glob`, `grep`,
  `diff`, `apply_patch`, `replace`, `lsp`, `lsp_read`, `git`, `git_read`,
  `commit`, `review`, `backend`) seed `unresolved_workspace_root()`
  (`src/tool/util.rs:39`) instead of the process cwd. `list`, `research`,
  and `skill` carry an `Option<PathBuf>` root instead of the sentinel; they
  resolve their root from the caller's execution context instead.
- `validate_path` / `validate_target_path` recognise that sentinel and return
  `MissingWorkspaceRoot` before canonicalizing. That single choke point is what
  makes every `allowed_root` consumer fail closed without each tool repeating
  the check.

`ToolError::MissingWorkspaceRoot` is deliberately distinct from
`ToolError::Execution` (a failure inside a rooted call) and `ToolError::Disabled`
(operator configuration): here the caller supplied no root at all, and the
remedy is to resolve one — never to widen the search. `ToolBroker` classifies
it as `missing_workspace_root`, and progress recovery treats it as a
deterministic tool error rather than a transport fault.

## Process admission is refused before the process exists (M005 C005)

The `Tool` trait's **default** `execute_structured` is the widest net for the
process-admission gate: it calls
`crate::tool::process_admission::admit_tool_process(self.name(), ctx.as_ref())`
*before* delegating to `execute()`
(`src/tool/mod.rs:194-212`, the gate call at `:209`). The gate classifies the tool against the
capability table and refuses an unqualified process surface at the tool
boundary — before the tool runs, and therefore before any process exists.

Two rules follow:

- A tool that **overrides** `execute_structured` must perform the same check.
  The default is deliberately the widest net so that a tool which starts a
  process cannot slip through by forgetting to consult the gate.
- The broker refuses the same surfaces at its own final dispatch, with a
  distinct typed error (`BrokerError::ProcessDenied`) rather than a generic
  execution failure. See `architecture/tool_broker.md`.

The gate is opt-in by construction: `ToolRegistryOptions.process_admission`
carries the resolved `SharedProcessAdmission` policy, and
`BrokerInvocationContext.process_admission` carries it per call.

## ToolContracts and the Canonical Broker

All production tool calls route through `ToolBroker` (`src/tool/broker.rs`),
which enforces a 10-step policy pipeline. Each tool has a `ToolContract`
describing caller policy, effect class, idempotency, retry/cache, and
projection policy. See [tool_broker.md](tool_broker.md).

## Security Considerations

### Repository retrieval workspace authority

Local repository retrieval is scoped from the accepted `ToolExecutionContext`
only when it carries an allowed decision, session, decision identifier and
workspace path-policy identity/revision. The root comes from that trusted
context's resolved `cwd`; model arguments never select it. Missing or
incomplete context sends an explicit disabled scope to the CodeGG-managed
Eggsearch child. The child authenticates private metadata and isolates its
bounded inventory view per root. This does not add file-level ACL semantics;
workspace path policy must authorize the selected root as a whole.

1. **Path validation**: All file paths validated before access
2. **Symlink protection**: `check_path_for_symlinks()` rejects symlinks
3. **Permission enforcement**: Tools check permissions before execution
4. **Canonical shell risk patterns**: Bash patterns provide advisory metadata for dangerous command shapes. They may contribute to one Ask through the existing approval router; they never provide OS containment or a guaranteed denial. Ordinary shell syntax such as wrappers, environment assignments, quoting, pipelines, and substitutions is not itself a risk signal. Explicit configured text denies remain best-effort invocation filters. Actual containment comes from the selected sandbox, child lease, and protected-resource checks.
5. **Canonical blocked commands**: Built-in command names are not a terminal blocklist. Explicit operator restrictions and narrow adapter restrictions remain best-effort text policy.
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

M008 qualification (`tests/agent_loop/reliability_qualification_m008.inc`) proves
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

Verified 2026-10-23 against source. The registration counts in this document
were re-derived from `src/tool/mod.rs` and reconciled with the
`architecture/overview.md` "Verified counts" table (measured 2026-10-23),
which is the single source of truth for counts. Corrected:

- Always-registered count: "39" → **40 distinct names**; the supporting
  "46 unconditional register sites" → **37 unconditional sites of 67 total**
  (54 `registry.register` + 13 `registry.register_boxed`). `todowrite` was
  moved out of the always-registered table into the todo conditional
  section: with both `todo_state` and `todo_policy` present and the policy
  refusing model todo writes, no todo tool is registered.
- `grep -c '\.register(' src/tool/mod.rs` = **55** (54 sites + the
  `catalog.register` call inside `ToolRegistry::register`), not 64.
- Registration loops `src/tool/mod.rs:935`/`:938` → `:1274`/`:1277`.
- `with_options()` `src/tool/mod.rs:408` → `:587`.
- `trait Tool` `:157-238` → `:163-256`; `ToolCategory` `:133-142` →
  `:139-150`.
- `ToolRegistry` `:249-258` → `:342-361`, and 8 → **12 fields** (added
  `decision_engine`, `decision_backend`, `advisor_mode`,
  `process_admission`).
- `ToolRegistryOptions` `:276` → `:376`, and 29 → **38 fields** (added
  `decision_engine`, `publication`, `windows_shell`, `git_read_only`,
  `notification_service`, `job_notification_service`, `ci_watch_service`,
  `research_provider`, `research_model`, `process_admission`).
- `execute_capture` `:1317-1349` → `:1701-1733`.
- `ToolCatalog` `src/tool/catalog.rs:170-178` → `:193-195`; the struct is now
  a single `inner: Arc<RwLock<ToolCatalogState>>` field, and the seven
  former fields live on the private `ToolCatalogState` (`:198-208`).
- Conditional registration blocks: memory `:617-626` → `:844-853`;
  extension `:627-634` → `:854-861`; work plan `:675-683` → `:902-909`.
- `python_script` name `src/python_script/tool.rs:444` → `:467`
  (`PythonScriptTool` struct at `:253`).
- The sentinel-seeding `Default` constructor list dropped `list`, `research`,
  and `skill` (none of them seeds `unresolved_workspace_root()`; the first
  carries `Option<PathBuf>` and the other two resolve their root from the
  execution context).

Added: the always-registered table gained the two missing unconditional
tools (`job`, `ci_watch`, both `ShellExec`); the always-registered section
now states the 75-name production surface, the 83 tool source files, and
the fact that `lsp_preview_apply` / `goal_*` / `work_order` are registered
by `build_session_tool_registry()` (`src/tool/factory.rs`) rather than by
`with_options()`; the "Where It Lives" tree gained the omitted modules
(`git_read.rs`, `lsp_read.rs`, `lsp_preview_apply.rs`, `goal.rs`, `job.rs`,
`ci_watch.rs`, `work_order.rs`, `work_plan.rs`, `disclosure.rs`,
`authorization.rs`, `process_admission.rs`).

Verified accurate: `disclosure_for()` at `src/tool/disclosure.rs:61`;
`tool_category_for_name()` at `src/permission/mod.rs:151`;
`unresolved_workspace_root()` / `validate_path` / `validate_target_path` /
`canonicalize_path` / `check_path_for_symlinks` at `src/tool/util.rs:39` /
`:72` / `:112` / `:284` / `:294`; `build_eggsact_tools()` at
`src/tool/deterministic.rs:106`; `UnrootedTool` and `DisabledTool` in
`src/tool/disabled.rs`; `ToolRegistry::register` / `register_boxed` at
`src/tool/mod.rs:1415`/`:1425`; the 4 `ToolCategory` variants; the
`ToolResult` shape; the 7 eggsearch wrappers; the 13 deterministic tools
(8 always-visible + 5 deferred); `multiedit` and `src/agent/team.rs` both
absent as documented; the `task` action list (`spawn_many`,
`wait_group`, `status_group`, `cancel_group`, `wait`, `cancel`,
`message`, `interrupt`, `converge`); and the `verify` action allow-list
(`check`, `build`, `lint`, `typecheck`, `format_check`, `auto`).

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

Corrected 2026-10-23: the production tool-name count was **75**, not 62. The
count is the `name()` values of the 67 production `impl Tool for` blocks under
`src/` **plus** the 13 `eggsact` deterministic validator names, which resolve
at runtime from `self.codegg_name` and so are invisible to a literal-name scan
of the impl blocks. They are counted instead at their `EggsactTool::new()`
call sites in `src/tool/deterministic.rs` — 8 always-visible and 5 deferred, as
`codegg doctor deterministic-tools` reports. Two runtime-named families were
checked and deliberately add nothing: `DisabledTool` carries the name of the
tool it replaces (`lsp`, `lsp_read`, `security`) and `UnrootedTool` forwards
its inner tool's name. Re-measure with `python3 scripts/measure_overview_counts.py`;
the method and the correction history are in
[count_audit_history.md](count_audit_history.md).
