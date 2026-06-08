# Native Tool Crates and MCP Adapter Handoff Plan

## Purpose

Move Codegg toward a library-first, MCP-second tool architecture.

The goal is not to turn Codegg into a mesh of local MCP subprocesses. The goal is to split durable tool domains into clean Rust crates that Codegg can call directly in-process, while optionally exposing those same crates through MCP adapter binaries for interoperability with other agents and for process isolation where it is actually useful.

This follows the pattern already established by the eggsearch integration: preserve stable native tool names for the model, route implementation through a backend abstraction, and keep raw MCP tool names hidden unless explicitly enabled.

## Current State Summary

Codegg currently has a direct tool registry in `src/tool/mod.rs`:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError>;
    fn category(&self) -> ToolCategory { ToolCategory::Mutating }
    fn set_available_tools(&mut self, _tools: Vec<String>) {}
    fn defer_loading(&self) -> bool { false }
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    catalog: catalog::ToolCatalog,
}
```

`ToolRegistry::with_defaults()` and `ToolRegistry::with_session_defaults()` both manually register the same broad set of built-in tools. The registered tools include filesystem tools, grep/list/glob, bash/terminal/git/commit, websearch/webfetch/research/codesearch, LSP, security, review, task/todo, skill, plan tools, image, batch, and tool_search.

MCP is implemented separately in `src/mcp/`. `McpService` supports local stdio servers, remote HTTP servers, OAuth, tool discovery, tool calls, and exposed names of the form `mcp__{server}__{tool}`.

The web search/fetch path already has a useful model for future extractions:

- Codegg exposes stable native tools: `websearch` and `webfetch`.
- `src/search_backend/` dispatches to `eggsearch`, `builtin`, or `disabled`.
- The default backend is eggsearch.
- Raw `mcp__eggsearch__*` tools are hidden unless explicitly exposed.
- The legacy built-in backend remains as an explicit fallback.

This plan generalizes that pattern for additional tool domains.

## Desired End State

Codegg should have three tool implementation layers:

```text
codegg-core
  ToolRegistry / ToolRouter
    NativeBackend        direct Rust crate calls, default for bundled hot-path tools
    McpBackend           stdio/http MCP calls, optional or external tools
    ShellBackend         tightly permissioned shell/process execution
```

From the model's perspective, stable Codegg tool names remain stable:

```text
read, write, edit, grep, glob, list, diff, apply_patch,
git, lsp, security, websearch, webfetch, research, task, todowrite, ...
```

From Codegg's perspective, implementation can be native or external:

```text
websearch -> Native wrapper -> eggsearch backend -> native crate or MCP adapter
lsp       -> Native wrapper -> egglsp crate
security  -> Native wrapper -> eggsec crate
repo_map  -> Native wrapper -> eggcontext crate
```

MCP should be an adapter target, not the internal architecture. Every extracted Codegg-owned tool project should expose a normal Rust library API first. MCP binaries are optional wrappers around that API.

## Non-Goals

Do not split the agent loop, model routing, provider invocation, session database, permission system, subagent scheduler, TUI, server mode, or compaction policy into MCP servers.

Do not make internal hot-path tools require subprocess MCP calls by default.

Do not expose both native wrappers and raw MCP versions of the same tool to the model by default.

Do not start by moving every tool into a separate repository. Begin with internal crate boundaries or workspace crates. Split into independent repos only when a crate has an independent user/release cadence.

Do not convert the `Tool` trait into a full MCP-shaped abstraction. MCP is one backend, not the core representation.

## Design Principles

1. Native by default for hot-path local tools.

Tools that are called frequently, run locally, return deterministic results, participate in context construction, or require tight permission coupling should be direct Rust calls.

Examples: grep/list/glob, diff, patch application, git status/diff facts, token counting, context packing, LSP diagnostics, deterministic security checks.

2. MCP by default for boundary-crossing tools.

Tools that are third-party, remote, credentialed, dependency-heavy, user-supplied, or isolation-worthy should use MCP or another process boundary.

Examples: web search, browser automation, GitHub/Jira/Linear, cloud APIs, databases, company docs, heavy scanners.

3. Crate-first for Codegg-owned tools.

Every Codegg-owned extraction should have:

```text
crates/<tool-domain>/src/lib.rs        stable Rust API
crates/<tool-domain>/src/types.rs      typed request/response structs
crates/<tool-domain>/src/error.rs      domain error type
crates/<tool-domain>/src/mcp.rs        optional MCP adapter helpers, if useful
crates/<tool-domain>/src/bin/...       optional CLI/MCP binary, if useful
```

4. Stable model-facing names.

The model should continue to see Codegg-native names. Backend names should not leak unless the user enables raw MCP exposure.

5. Permission remains centralized.

The Codegg permission checker remains authoritative. Extracted crates may classify operations, but they must not independently bypass or weaken Codegg policy.

6. Output provenance must be explicit.

Every backend should report enough provenance for logs, debugging, and future UI display: backend kind, crate/server name, version if available, elapsed time, truncation status, and trust framing.

## Phase 1: Introduce a Backend-Aware Tool Execution Contract

Add a small internal abstraction without changing all tools at once.

Suggested new module:

```text
src/tool/backend.rs
```

Suggested types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolBackendKind {
    Native,
    Mcp,
    Shell,
    BuiltinLegacy,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    pub backend: ToolBackendKind,
    pub session_id: Option<String>,
    pub cwd: std::path::PathBuf,
    pub permission_mode: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolProvenance {
    pub backend: String,
    pub implementation: String,
    pub version: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub truncated: bool,
    pub trust: ToolTrust,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTrust {
    LocalTrusted,
    LocalUntrusted,
    ExternalUntrusted,
    MutatingSideEffect,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StructuredToolResult {
    pub output: String,
    pub success: bool,
    pub provenance: Option<ToolProvenance>,
}
```

Do not force all existing tools to return `StructuredToolResult` in the first pass. Instead, add compatibility helpers:

```rust
impl StructuredToolResult {
    pub fn legacy(tool_name: &str, output: String) -> Self { ... }
    pub fn into_legacy_output(self) -> String { self.output }
}
```

Then add an optional method to `Tool` with a default implementation:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    ...
    async fn execute_structured(
        &self,
        input: serde_json::Value,
        _ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute(input).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}
```

Keep existing `execute()` callers working. New wrappers can opt into structured execution.

Acceptance criteria:

- Existing tests compile without requiring every tool to change.
- `ToolResult` remains backward compatible.
- At least one tool, ideally `websearch` or `webfetch`, returns structured provenance internally while still surfacing the same string output to the model.

## Phase 2: Remove Default Registry Duplication

`with_defaults()` and `with_session_defaults()` currently duplicate most registrations. This will become fragile as tools gain backend configuration.

Add a builder-like helper:

```rust
pub struct ToolRegistryOptions {
    pub todo_state: Option<Arc<Mutex<TodoState>>>,
    pub todo_policy: Option<TaskStatePolicy>,
    pub pool: Option<SqlitePool>,
    pub session_id: Option<String>,
    pub lsp_service: Option<Arc<LspService>>,
    pub tool_backends: ToolBackendConfig,
}

impl ToolRegistry {
    pub fn with_options(options: ToolRegistryOptions) -> Self { ... }
}
```

Then implement:

```rust
pub fn with_defaults() -> Self {
    Self::with_options(ToolRegistryOptions::default())
}

pub fn with_session_defaults(...) -> Self {
    Self::with_options(ToolRegistryOptions { ... })
}
```

Acceptance criteria:

- There is one authoritative registration sequence.
- Todo read/write session-specific behavior is preserved.
- LSP service construction is injectable instead of hardcoded in two places.
- The default registry remains stable from the model's perspective.

## Phase 3: Define the Native/MCP Backend Selection Pattern

Generalize the eggsearch pattern.

Add a generic config shape that future tools can reuse without over-abstracting everything:

```rust
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolImplementationBackend {
    Native,
    Mcp,
    Builtin,
    Disabled,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default, PartialEq)]
#[serde(default)]
pub struct ExternalToolBackendConfig {
    pub backend: Option<ToolImplementationBackend>,
    pub expose_raw_mcp_tools: Option<bool>,
    pub fallback_to_native: Option<bool>,
    pub server_name: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub timeout_ms: Option<u64>,
    pub env: Option<HashMap<String, String>>,
}
```

Do not immediately replace `SearchConfig`; it is already specific and working. Instead, use this generic shape for new domains and migrate search only later if there is a clear benefit.

Candidate config sections:

```toml
[lsp_backend]
backend = "native"       # native | mcp | disabled
fallback_to_native = true
expose_raw_mcp_tools = false

[security_backend]
backend = "native"       # native | mcp | disabled

[context_backend]
backend = "native"       # native | mcp | disabled
```

Acceptance criteria:

- No behavioral change yet.
- Config parsing tests cover default backend selection.
- Unknown or disabled backend states produce clear errors at call time.

## Phase 4: Extract the LSP Domain First (`egglsp` Candidate)

LSP is the strongest first extraction candidate because it already has a rich service layer and a heavy dependency surface.

Current Codegg module:

```text
src/lsp/
  client.rs
  download.rs
  language.rs
  operations.rs
  root.rs
  server.rs
  service.rs
```

Target internal crate first:

```text
crates/egglsp/
  Cargo.toml
  src/lib.rs
  src/client.rs
  src/download.rs
  src/language.rs
  src/operations.rs
  src/root.rs
  src/server.rs
  src/service.rs
  src/types.rs
  src/error.rs
```

Move generic LSP logic into `egglsp`. Keep Codegg-specific tool wrapper in `src/tool/lsp.rs`.

Codegg wrapper should become thin:

```rust
pub struct LspTool {
    service: Arc<egglsp::LspService>,
}
```

The `egglsp` crate should not depend on Codegg config types. Replace `crate::config::schema::LspConfig` and `LspRule` with crate-local config types. Codegg should implement conversion:

```rust
impl From<crate::config::schema::LspConfig> for egglsp::LspConfig { ... }
```

Required APIs:

```rust
pub struct LspService { ... }

impl LspService {
    pub fn new(config: LspConfig) -> Self;
    pub async fn diagnostics(&self, file: &Path) -> Result<DiagnosticsResponse, LspError>;
    pub async fn definition(&self, file: &Path, line: u32, character: u32) -> Result<LocationResponse, LspError>;
    pub async fn references(&self, file: &Path, line: u32, character: u32) -> Result<Vec<Location>, LspError>;
    pub async fn document_symbols(&self, file: &Path) -> Result<SymbolResponse, LspError>;
    pub async fn workspace_symbols(&self, query: &str, root: &Path) -> Result<SymbolResponse, LspError>;
}
```

Optional later MCP binary:

```text
egglsp mcp stdio
```

MCP tools:

```text
lsp_diagnostics
lsp_definition
lsp_references
lsp_document_symbols
lsp_workspace_symbols
```

Acceptance criteria:

- `codegg` still exposes a single native `lsp` tool by default.
- No raw `mcp__egglsp__*` tools are exposed by default.
- Existing LSP tests pass after moving imports.
- `egglsp` has unit tests independent of Codegg.
- Codegg can construct `egglsp::LspService` from existing config.

## Phase 5: Extract Git/Diff/Worktree Facts (`egggit` Candidate)

Current state includes separate pieces in `src/tool/git.rs`, `src/tool/commit.rs`, `src/tool/review.rs`, `src/worktree/`, and an orphaned `src/git/` noted in the architecture docs. Consolidate deterministic git/repo facts before changing agent-facing tools.

Target crate:

```text
crates/egggit/
  Cargo.toml
  src/lib.rs
  src/status.rs
  src/diff.rs
  src/patch.rs
  src/worktree.rs
  src/history.rs
  src/types.rs
  src/error.rs
```

Start with read-only and deterministic operations:

```rust
pub async fn repo_status(root: &Path) -> Result<RepoStatus, EgggitError>;
pub async fn diff_summary(root: &Path, base: Option<&str>) -> Result<DiffSummary, EgggitError>;
pub async fn changed_files(root: &Path, base: Option<&str>) -> Result<Vec<ChangedFile>, EgggitError>;
pub async fn file_diff(root: &Path, path: &Path, base: Option<&str>) -> Result<FileDiff, EgggitError>;
pub async fn validate_patch(root: &Path, patch: &str) -> Result<PatchValidation, EgggitError>;
```

Do not move commit creation first. Commit creation is mutating, permission-sensitive, and currently coupled to LLM-generated commit messages. Leave the `commit` tool in Codegg until the read-only git substrate is stable.

Then refactor:

- `review` should consume `egggit::diff_summary()` rather than shelling out or duplicating diff logic.
- `commit` should consume `egggit` facts but keep mutation inside Codegg permission flow.
- `git` tool may remain a low-level command wrapper, but higher-level tools should prefer `egggit` APIs.
- `worktree` operations may move later after read-only facts are stable.

Optional MCP tools:

```text
git_status
git_changed_files
git_diff_summary
git_file_diff
patch_validate
```

Acceptance criteria:

- No change to model-facing `git`, `commit`, or `review` tool names.
- Review and commit message generation use the same deterministic diff facts.
- The orphaned `src/git/` state is resolved: either removed, wired into `egggit`, or documented as intentionally obsolete.
- Mutating git operations remain permission-gated in Codegg.

## Phase 6: Extract Context Packing (`eggcontext` Candidate)

This is the most important extraction for long sessions, subagents, deep research, and token efficiency.

Target crate:

```text
crates/eggcontext/
  Cargo.toml
  src/lib.rs
  src/token.rs
  src/chunk.rs
  src/repo_map.rs
  src/rank.rs
  src/markdown.rs
  src/transcript.rs
  src/types.rs
  src/error.rs
```

Initial APIs:

```rust
pub fn count_tokens(model_hint: Option<&str>, text: &str) -> Result<usize, EggcontextError>;
pub fn chunk_text(input: ChunkRequest) -> Result<Vec<TextChunk>, EggcontextError>;
pub fn chunk_code(input: CodeChunkRequest) -> Result<Vec<CodeChunk>, EggcontextError>;
pub async fn build_repo_map(input: RepoMapRequest) -> Result<RepoMap, EggcontextError>;
pub fn rank_context(input: RankContextRequest) -> Result<RankedContext, EggcontextError>;
pub fn compact_transcript_deterministic(input: TranscriptCompactRequest) -> Result<TranscriptCompactResult, EggcontextError>;
```

Codegg keeps compaction policy and model-driven compaction in `agent/compaction.rs`. `eggcontext` only provides deterministic transforms and token accounting.

Refactor targets:

- Agent compaction should use `eggcontext` for deterministic pre-pass and token estimation.
- Research should use `eggcontext` for document chunking and dedup.
- Subagents should receive context bundles built by `eggcontext` rather than ad hoc file/text selection.
- `tool_search` and deferred tools can eventually use `eggcontext` summaries for better discovery.

Optional MCP tools:

```text
count_tokens
chunk_text
build_repo_map
rank_context
compact_transcript_deterministic
```

Acceptance criteria:

- Existing compaction behavior remains available.
- Deterministic compaction can be enabled independently of model compaction.
- Token counting and context packing are testable without booting Codegg.
- Context bundle output includes provenance: source files, byte ranges, token estimates, truncation.

## Phase 7: Extract Deterministic Security Checks (`eggsec` Candidate)

Current `security` tool should become a native wrapper over a deterministic security crate. Keep any LLM-based security review as a higher-level Codegg workflow, not as the core scanner.

Target crate:

```text
crates/eggsec/
  Cargo.toml
  src/lib.rs
  src/secrets.rs
  src/commands.rs
  src/dependencies.rs
  src/rust.rs
  src/web.rs
  src/types.rs
  src/error.rs
```

Initial scan classes:

```text
secret_pattern_scan
suspicious_command_scan
path_traversal_pattern_scan
sql_injection_pattern_scan
unsafe_rust_scan
build_script_scan
manifest_dependency_inventory
```

Initial APIs:

```rust
pub async fn scan_files(input: FileScanRequest) -> Result<SecurityScanResult, EggsecError>;
pub async fn scan_diff(input: DiffScanRequest) -> Result<SecurityScanResult, EggsecError>;
pub async fn scan_manifests(input: ManifestScanRequest) -> Result<DependencyInventory, EggsecError>;
```

Codegg responsibilities that remain in core:

- Decide when the scan runs.
- Decide whether findings block tool execution.
- Decide whether to invoke a security subagent.
- Present findings in TUI/server events.
- Enforce permissions.

Optional MCP tools:

```text
security_scan_files
security_scan_diff
security_scan_manifests
```

Acceptance criteria:

- `security` remains a native Codegg tool name.
- Deterministic findings include rule IDs, severity, file/range, confidence, and remediation text.
- Findings are stable enough for snapshot tests.
- No network calls are required for the initial scanner.

## Phase 8: Keep Filesystem/Edit Tools In-Core for Now

Do not extract `read`, `write`, `edit`, `apply_patch`, `bash`, or `terminal` in the first wave.

Reasoning:

- These are deeply coupled to permissions, sandboxing, sensitive path rules, event streaming, snapshots/checkpoints, and UI feedback.
- Moving them too early risks weakening safety and creating confusing backend semantics.
- They are hot-path tools where subprocess/MCP overhead is undesirable.

However, `diff`, `glob`, `grep`, and `list` can gradually consume shared helper crates:

- `eggcontext` for ranking/context packaging.
- `egggit` for git-aware change facts.
- `eggsact` for deterministic text equivalence/diff sanity.

Only revisit full extraction after the backend contract and provenance model are stable.

## Phase 9: Normalize MCP Raw Tool Exposure

Apply the eggsearch raw-tool hiding pattern globally.

Add policy:

```toml
[mcp]
expose_raw_tools = false          # default for Codegg-owned backend MCPs
expose_third_party_tools = true   # existing behavior for user-configured MCPs
```

Or more explicitly per server:

```toml
[mcp.egglsp]
managed_by_codegg = true
expose_raw_tools = false

[mcp.github]
managed_by_codegg = false
expose_raw_tools = true
```

Rules:

- Codegg-owned backend MCP servers should be hidden by default when there is a native wrapper.
- User-configured third-party MCP servers should remain visible unless disabled.
- If a native wrapper delegates to MCP, raw tools should not duplicate the model-facing API.
- Tool catalog should record hidden backend tools for diagnostics but not include them in provider tool definitions.

Acceptance criteria:

- `McpService::list_tools()` or its caller can filter raw tools by server policy.
- `/mcps` still shows hidden managed servers for diagnostics.
- Tool search does not surface hidden duplicate backend tools unless explicitly configured.

## Phase 10: Add a Tool Backend Diagnostics Surface

Add a command or status section that makes the new architecture observable.

Possible slash command:

```text
/tools
/tool-backends
```

Output should include:

```text
Tool         Backend   Implementation    Status       Raw MCP exposed
websearch    MCP       eggsearch          ready        no
webfetch     MCP       eggsearch          ready        no
lsp          Native    egglsp             ready        n/a
security     Native    eggsec             ready        n/a
git          Native    codegg/egggit      ready        n/a
```

Also include warnings:

```text
- eggsearch configured but unavailable; websearch will error unless fallback is enabled.
- lsp backend disabled; lsp tool hidden.
- raw mcp__eggsearch__web_search hidden because native websearch wrapper is active.
```

Acceptance criteria:

- Diagnostics distinguish unavailable, disabled, native, MCP, and fallback states.
- Diagnostics do not leak secrets or environment variable values.
- Diagnostics are available in exec mode or logs as well as the TUI if feasible.

## Phase 11: Testing Strategy

Add tests at three levels.

1. Crate-level tests.

Each extracted crate should have unit tests independent of Codegg.

Examples:

```text
eggcontext: chunking, token counting, repo map ranking
egggit: parse status/diff fixtures, validate patch fixtures
eggsec: scan fixtures with expected rule IDs
egglsp: language/root/server selection tests; mock LSP client tests
```

2. Codegg wrapper tests.

Each native wrapper should have tests that mock the backend and verify:

- model-facing tool name is stable;
- parameters schema is stable;
- permissions category is correct;
- backend errors produce actionable messages;
- provenance is recorded;
- disabled backend hides or errors cleanly.

3. MCP adapter tests.

For optional MCP binaries:

- initialize;
- list tools;
- call each tool with a fixture;
- verify error shape;
- verify output clamping/trust framing where applicable.

Acceptance criteria:

- No extraction is complete until the wrapper and crate have tests.
- Snapshot tests cover tool schema stability.
- Failure-path tests cover missing binaries, disabled backend, malformed input, timeout, and fallback behavior.

## Phase 12: Suggested Implementation Order

Do not attempt all extractions in one pass.

Recommended sequence:

1. Add `tool/backend.rs` and structured provenance compatibility.
2. Deduplicate `ToolRegistry` registration with `ToolRegistryOptions`.
3. Add backend diagnostics and raw MCP exposure policy.
4. Extract `egglsp` as an internal workspace crate.
5. Extract `egggit` read-only facts.
6. Extract `eggcontext` deterministic token/context utilities.
7. Extract `eggsec` deterministic scanning.
8. Revisit MCP adapters for each extracted crate after native usage is stable.

Each phase should compile and preserve the existing model-facing tool surface.

## Specific Implementation Notes

### Tool categories must remain conservative

Unknown backend tools should default to `Mutating`, not `ReadOnly`. Preserve the existing conservative behavior for unknown tool names.

### Avoid global state where possible

`src/search_backend/state.rs` was acceptable for eggsearch integration, but do not copy that pattern broadly unless necessary. Prefer explicit backend handles injected through `ToolRegistryOptions`.

### Keep Codegg config conversion one-way

Extracted crates should not import Codegg config types. Codegg should convert its config into crate-local config structs.

### Keep model schemas stable

Add snapshot tests around `ToolRegistry::definitions()`. Tool descriptions and JSON schemas are part of the model contract. Avoid large noisy schema changes unless intentional.

### Prefer narrow APIs over shell passthroughs

`egggit` should expose `repo_status`, `changed_files`, `diff_summary`, etc. It should not simply become a generic `run_git_command` wrapper. Generic command execution belongs in Codegg's permissioned shell/git tool.

### Treat MCP outputs as untrusted when external

Any backend crossing a process/network boundary should report `ToolTrust::ExternalUntrusted` unless Codegg explicitly classifies it otherwise.

### Do not over-abstract before the second extraction

The first extraction may reveal where the backend abstraction is too weak or too heavy. Keep Phase 1 minimal, then generalize after `egglsp` and `egggit` have both used the pattern.

## Done Criteria for the Overall Initiative

This initiative is complete when:

- Codegg has a documented native/MCP backend policy.
- Tool registration is centralized and backend-aware.
- At least two substantial tool domains have been moved to native crates without changing model-facing tool names.
- Raw Codegg-managed MCP duplicate tools are hidden by default.
- Backend diagnostics show native/MCP/fallback/disabled status.
- Permission gating remains centralized in Codegg.
- Extracted crates have independent tests and do not depend on Codegg internals.
- Codegg still works as a single binary with native defaults and does not require local MCP subprocesses for hot-path coding tools.
