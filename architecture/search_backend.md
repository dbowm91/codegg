# Search Backend Module

The `search_backend` module is the wrapper layer between Codegg's
agent-facing `websearch` and `webfetch` tools and the underlying
provider. The default backend is the external `eggsearch` MCP
server; the legacy in-tree implementation under `src/search/*` is
retained as an explicit fallback.

## Module layout

```
src/search_backend/
├── mod.rs           # Public dispatch entry points
├── state.rs         # Process-global McpService + SearchConfig slots
├── bootstrap.rs     # Connect eggsearch at startup; emit BootstrapReport
├── eggsearch.rs     # Adapter: native args -> eggsearch MCP args
├── legacy.rs        # Adapter: native args -> in-tree SearchProviderRegistry
└── framing.rs       # external_untrusted framing + clamp_output
```

## Public surface

```rust
// src/search_backend/mod.rs
pub async fn dispatch_web_search(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_web_fetch(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_repo_search(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_repo_fetch(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_repo_map(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_security_search(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_research_search(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_batch_fetch(input: &Value) -> Result<String, ToolError>;
pub async fn dispatch_evidence_bundle(input: &Value) -> Result<String, ToolError>;
```

The native tools at `src/tool/websearch.rs`,
`src/tool/webfetch.rs`, and the seven eggsearch wrapper tools
(`repo_search`, `repo_fetch`, `repo_map`, `security_search`,
`research_search`, `batch_fetch`, `evidence_bundle`) call these
directly. The dispatch resolves the configured `SearchConfig` from
`state::search_config()` and forwards to the eggsearch adapter.

The original two (`dispatch_web_search`, `dispatch_web_fetch`) support
`backend = "builtin"` fallback. The seven new dispatch functions
require `backend = "eggsearch"` and return an error otherwise.

## State management

`src/search_backend/state.rs` exposes two process-global slots
backed by `std::sync::RwLock<Option<...>>` (despite an older
docstring calling them "OnceLock-style"):

```rust
pub fn install_mcp_service(svc: Arc<RwLock<McpService>>);
pub fn mcp_service() -> Option<Arc<RwLock<McpService>>>;
pub fn install_search_config(cfg: SearchConfig);
pub fn search_config() -> SearchConfig;
```

The slots are populated at startup by
`bootstrap::bootstrap_search_backend`. Tool execution reads them
later. The values are installed once in production; tests can
re-install them to override the config and the `McpService`
between cases. Production code should treat the slots as
immutable after startup.

## Bootstrap

`bootstrap::bootstrap_search_backend(config)` is called from
`main.rs`, `tui/mod.rs`, `exec.rs`, and `core/daemon.rs`. It:

1. Returns the existing service if one is already installed
   (idempotent for re-entry).
2. Calls `bootstrap_eggsearch(config)` which:
   - Resolves the effective `SearchConfig`.
   - Installs it into the state slot.
   - Skips MCP setup unless `backend = "eggsearch"`.
   - If the user has an explicit `[mcp.eggsearch]` block, uses
     `connect_from_config` to honor it.
   - Otherwise spawns `eggsearch` via `McpService::connect_stdio`.
   - Lists the tools it advertises for the `BootstrapReport`.

The returned `BootstrapReport` is consumed by the doctor command
(`codegg doctor search`).

## Adapter contracts

### `eggsearch::call_web_search(server, input, max_chars)`

- Reads `query` (required, non-empty).
- Reads `num_results` (or alias `max_results`); default 8, capped
  at 30.
- Reads `provider` and maps known hints to a `providers` list
  (see `translate_provider_hint`). Unknown hints let eggsearch
  auto-pick.
- Calls `McpService::call_tool(server, "web_search", args)` with a
  60s timeout.
- Clamps output to `max_search_output_chars` and wraps in
  `frame_search_results`.

### `eggsearch::call_web_fetch(server, input, max_chars)`

- Reads `url` (required).
- Reads `max_length` (or alias `max_chars`); default 10_000.
- Always sends `extract_mode = "text"`, `include_links = false`.
- Calls `McpService::call_tool(server, "web_fetch", args)`.
- Clamps output to `max_fetch_output_chars` and wraps in
  `frame_fetched_page`.

### `eggsearch::call_repo_search(server, input, max_chars)`

- Reads `query` (required, non-empty).
- Reads optional `num_results` (default 8, max 30).
- Calls `call_tool(server, "repo_search", args)` with 60s timeout.
- Clamps to `max_repo_output_chars` (default 15k), frames with
  `frame_repo_results`.

### `eggsearch::call_repo_fetch(server, input, max_chars)`

- Reads `url` or `repo`+`path` (required).
- Reads optional `max_length` (default 10k).
- Calls `call_tool(server, "repo_fetch", args)` with 60s timeout.
- Clamps to `max_repo_output_chars`, frames with `frame_repo_file`.

### `eggsearch::call_repo_map(server, input, max_chars)`

- Reads `repo` (required).
- Reads optional `path` (default root).
- Calls `call_tool(server, "repo_map", args)` with 60s timeout.
- Clamps to `max_repo_output_chars`, frames with `frame_repo_map`.

### `eggsearch::call_security_search(server, input, max_chars)`

- Reads `query` (required, non-empty).
- Calls `call_tool(server, "security_search", args)` with 60s timeout.
- Clamps to `max_security_output_chars` (default 10k), frames with
  `frame_security_results`.

### `eggsearch::call_research_search(server, input, max_chars)`

- Reads `query` (required, non-empty).
- Reads optional `num_results` (default 8, max 30).
- Calls `call_tool(server, "research_search", args)` with 60s timeout.
- Clamps to `max_research_output_chars` (default 15k), frames with
  `frame_research_results`.

### `eggsearch::call_batch_fetch(server, input, max_chars)`

- Reads `urls` (required, non-empty array).
- Reads optional `max_length_per_url` (default 10k).
- Calls `call_tool(server, "batch_fetch", args)` with 60s timeout.
- Clamps to `max_batch_output_chars` (default 50k), frames with
  `frame_batch_results`.

### `eggsearch::call_build_evidence_bundle(server, input, max_chars)`

- Reads `sources` (required, array of source descriptors).
- Calls `call_tool(server, "build_evidence_bundle", args)` with 60s timeout.
- Clamps to `max_evidence_output_chars` (default 100k), frames with
  `frame_evidence_bundle`.

### `legacy::call_web_search_legacy(registry, input, max_chars, timeout)`

- Uses `SearchProviderRegistry::from_env()` to pick a provider.
- Errors with a clear "no websearch provider configured" message
  if no providers are configured in env.
- Returns a formatted hit list, capped at `max_chars`.

## Hiding raw MCP tools

The agent loop's `build_tool_definitions` filters out tools whose
name starts with `mcp__<server_name>__` from the model prompt
when `expose_raw_mcp_tools = false` (the default). The server
name is resolved from the `SearchConfig` so custom names are
honored. The filter lives near the top of the MCP tool handling
block in `src/agent/loop.rs` (the line range drifts; search for
`expose_raw_mcp_tools` and the `mcp__` prefix filter for the
exact location).

## Trust framing

Every eggsearch result is wrapped before returning to the model.
See `framing.rs`:

```text
[external_web_content trust=external_untrusted source=eggsearch tool=websearch]
<result>
[/external_web_content]
```

The fetch frame is stronger and includes an explicit
"EXTERNAL, UNTRUSTED DATA" warning since fetched pages can
contain arbitrary attacker-controlled text. Codegg's framing is
redundant with any internal eggsearch labeling; the redundancy
keeps the trust boundary stable even if eggsearch changes its
output format.

## Config

```toml
[search]
backend = "eggsearch"           # "eggsearch" | "builtin" | "disabled"
expose_raw_mcp_tools = false
fallback_to_builtin = false
max_search_output_chars = 12000
max_fetch_output_chars = 20000
max_repo_output_chars = 15000
max_security_output_chars = 10000
max_research_output_chars = 15000
max_batch_output_chars = 50000
max_evidence_output_chars = 100000

[search.eggsearch]
enabled = true
server_name = "eggsearch"
command = "eggsearch"
args = ["mcp", "stdio"]
timeout_ms = 60000

[search.eggsearch.env]
BRAVE_SEARCH_API_KEY = "$BRAVE_SEARCH_API_KEY"
```

## Doctor

```bash
codegg doctor search
```

Output is a `BootstrapReport::summary_lines()` dump covering:
backend, command, MCP connection status, advertised tools,
`expose_raw_mcp_tools`, `fallback_to_builtin`, and output caps.

## Where to add new providers

New web search providers should be added in the eggsearch
project, not in Codegg's built-in search provider registry
(`src/search/`). The built-in registry is legacy fallback only.
Codegg owns the wrapper UX, permissioning, output caps, trust
framing, and backend selection; the actual search/fetch logic
lives in eggsearch.

## Why this design?

- **Why do `websearch` and `webfetch` still exist in Codegg?**
  They are stable native tool names. Codegg owns the wrapper UX,
  permissioning, output caps, trust framing, and backend
  selection. The actual search/fetch logic lives in eggsearch.
- **Why does Codegg also have generic MCP support?**
  The general MCP infrastructure connects arbitrary MCP servers
  (file system, git, db, etc.). Eggsearch is just one consumer
  of that infrastructure.
- **Why are raw eggsearch MCP tools hidden by default?**
  To keep the model's tool surface stable and prevent it from
  bypassing the native wrapper's framing, output caps, and
  permission rules.
- **Where should a new web search provider be added?**
  In eggsearch, not in Codegg.
- **What happens if eggsearch is missing?**
  `codegg doctor search` reports it as unavailable. With
  `backend = "eggsearch"`, `websearch`/`webfetch` return an
  actionable error. With `fallback_to_builtin = true`, they fall
  back to the legacy implementation.

## See also

- [tool.md](tool.md) – the `websearch`, `webfetch`, and eggsearch wrapper tools
- [mcp.md](mcp.md) – the `McpService` plumbing
- [config.md](config.md) – config loading and validation
- [security.md](security.md) – SSRF protection
