# Phase 1: Eggsearch Wrapper Surface Expansion

## Goal

Expand Codegg's existing eggsearch integration from `websearch` and `webfetch` only into a stable native wrapper surface for the rest of the high-value eggsearch MCP tools. The model should continue calling Codegg-native tools rather than raw `mcp__eggsearch__...` tools.

## Scope

Add Codegg-native wrappers for:

- `repo_search`
- `repo_fetch`
- `repo_map`
- `security_search`
- `research_search`
- `batch_fetch`
- `build_evidence_bundle`

Retain the current `websearch` and `webfetch` wrappers and avoid behavior regressions there.

## Design constraints

- Do not expose raw eggsearch MCP tools by default.
- Do not route these calls through shell execution.
- Do not make eggsearch a direct Rust dependency in Codegg for this phase.
- Do not let the model bypass Codegg output caps or trust framing.
- Preserve existing fallback behavior for `websearch` and `webfetch`.
- Treat all remote/web/advisory/repository evidence as instruction-untrusted.

## Implementation steps

### 1. Extend `src/search_backend/eggsearch.rs`

Add adapter functions next to the existing `call_web_search`, `call_web_fetch`, and `call_provider_status` functions.

Suggested functions:

```rust
pub async fn call_repo_search(server: &str, input: &Value, max_output_chars: usize) -> Result<String, ToolError>;
pub async fn call_repo_fetch(server: &str, input: &Value, max_output_chars: usize) -> Result<String, ToolError>;
pub async fn call_repo_map(server: &str, input: &Value, max_output_chars: usize) -> Result<String, ToolError>;
pub async fn call_security_search(server: &str, input: &Value, max_output_chars: usize) -> Result<String, ToolError>;
pub async fn call_research_search(server: &str, input: &Value, max_output_chars: usize) -> Result<String, ToolError>;
pub async fn call_batch_fetch(server: &str, input: &Value, max_output_chars: usize) -> Result<String, ToolError>;
pub async fn call_build_evidence_bundle(server: &str, input: &Value, max_output_chars: usize) -> Result<String, ToolError>;
```

Each adapter should:

- Validate required arguments before calling MCP.
- Normalize Codegg-native aliases into eggsearch MCP argument names.
- Apply a timeout.
- Convert MCP call errors into actionable `ToolError::Execution` or `ToolError::Timeout` variants.
- Clamp output with a domain-appropriate cap.
- Wrap output in a domain-specific trust frame.

### 2. Add public dispatch functions in `src/search_backend/mod.rs`

Add dispatch entry points analogous to `dispatch_web_search` and `dispatch_web_fetch`.

Suggested functions:

```rust
pub async fn dispatch_repo_search(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_repo_fetch(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_repo_map(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_security_search(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_research_search(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_batch_fetch(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_evidence_bundle(input: &Value) -> Result<String, ToolError>;
```

For now, these can require `backend = "eggsearch"`. Unlike `websearch` and `webfetch`, there may not be a meaningful built-in fallback for every expanded tool. If fallback is not available, return a clear unavailable error rather than silently degrading.

### 3. Add Codegg-native tool modules

Add modules under `src/tool/` such as:

- `repo_search.rs`
- `repo_fetch.rs`
- `repo_map.rs`
- `security_search.rs` or integrate with existing `security.rs` carefully
- `research_search.rs` or integrate with existing `research.rs` carefully
- `batch_fetch.rs`
- `evidence_bundle.rs`

Each module should implement `Tool` with:

- A stable Codegg-native name.
- A concise description that states the trust boundary.
- A JSON schema that is narrower and more model-friendly than raw MCP if needed.
- `ToolCategory::ReadOnly`.
- `execute` delegating to the corresponding dispatch function.
- `execute_structured` attaching provenance with backend `mcp`, implementation `eggsearch/<tool>`, and trust `external_untrusted` unless the adapter can prove a local-only source.

### 4. Register tools conservatively

Register the new wrappers in `ToolRegistry::with_options` near the existing web/search/research tools. Consider deferring heavier tools by default:

- Model-visible by default: `repo_search`, `repo_fetch`, `security_search`, `research_search`.
- Deferred/contextual: `repo_map`, `batch_fetch`, `evidence_bundle`.

Preserve `tool_search` compatibility by ensuring deferred names are discoverable.

### 5. Preserve raw MCP filtering

Verify the agent loop still filters raw `mcp__eggsearch__...` definitions when `expose_raw_mcp_tools = false`. Expand tests if the filter currently only accounts for `web_search` and `web_fetch` by assumption.

### 6. Update bootstrap reporting only if needed

Do not require all expanded tools to be present for Codegg to start. The bootstrap report should list advertised tools, but individual wrapper calls should fail with a precise message if eggsearch is old or missing a tool.

## Tool schema guidance

Use Codegg-native names and arguments that are stable and compact.

Examples:

`repo_search`:

- `query`: required string.
- `repo`: optional repository locator.
- `language`: optional string.
- `max_results`: optional integer, capped.
- `include_snippets`: optional boolean.

`repo_fetch`:

- `repo`: required or derivable repository locator.
- `path`: required string.
- `start_line`: optional integer.
- `end_line`: optional integer.
- `symbol`: optional string.

`security_search`:

- `query`: required string.
- `ecosystem`: optional string.
- `package`: optional string.
- `cve`: optional string.
- `max_results`: optional integer, capped.

`research_search`:

- `query`: required string.
- `domains`: optional array of provider/domain hints.
- `max_results`: optional integer, capped.

`batch_fetch`:

- `urls`: optional array of explicit HTTP(S) URLs.
- `items`: optional array of repo/file locators if eggsearch supports them.
- `max_chars_per_item`: optional integer, capped.

`evidence_bundle`:

- Accept structured input produced by prior search/fetch calls.
- Keep schema conservative until eggsearch's exact contract is verified.

## Validation

Add tests for:

- Adapter argument translation for each new wrapper.
- Missing required arguments.
- Timeout conversion.
- Missing eggsearch service.
- Missing eggsearch MCP tool.
- Output clamping.
- Trust frame presence.
- `ToolCategory::ReadOnly` classification.
- `execute_structured` provenance fields.
- Raw MCP tools hidden by default.

Use mock `McpService` or existing MCP test support where possible. Do not require live network access for default tests.

## Acceptance criteria

- Codegg registers native wrappers for the expanded eggsearch surface.
- The model can call the new Codegg-native tools without seeing raw MCP names.
- All outputs are capped and trust-framed.
- `/tool-backends` or equivalent structured execution reporting identifies eggsearch as the backend.
- `cargo fmt`, `cargo clippy`, and `cargo test` pass under the repo's normal gate.

## Risks

The main risk is schema drift between Codegg wrappers and eggsearch MCP tools. Keep all translation code isolated in `src/search_backend/eggsearch.rs`, add schema translation tests, and return actionable errors when the upstream tool contract changes.
