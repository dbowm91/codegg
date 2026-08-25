# Search Backend Module

Wrapper layer between Codegg's agent-facing search/evidence tools and
the underlying search provider. The default backend is the external
`eggsearch` MCP server; the legacy in-tree implementation under
`src/search/*` is retained only as an explicit fallback.

## Purpose

Codegg exposes stable native tool names (`websearch`, `webfetch`,
`repo_search`, `repo_fetch`, `repo_map`, `security_search`,
`research_search`, `batch_fetch`, `evidence_bundle`) to the agent.
Internally they delegate to a pluggable backend. The search backend
module owns the dispatch logic, output framing, truncation, and
trust boundary between external content and the model.

`eggsearch` is the sole normal owner of external search-provider
execution. The `codesearch` model name is a compatibility alias over
eggsearch `repo_search` with `profile = "coding"`. Deep research uses
the shared eggsearch `research_search`/`security_search` boundary to
collect external evidence.

## Where It Lives

```
src/search_backend/
├── mod.rs           # Public dispatch_* entry points, StructuredSearchResult
├── state.rs         # Process-global McpService + SearchConfig slots
├── bootstrap.rs     # Connect eggsearch at startup; emit BootstrapReport
├── eggsearch.rs     # Adapter: native args → eggsearch MCP args
├── legacy.rs        # Adapter: native args → in-tree SearchProviderRegistry
├── framing.rs       # external_untrusted framing + clamp_output
└── test_support.rs  # Cross-process flock for test isolation
```

## How It Works

### State Management (`state.rs`)

Two process-global slots backed by `std::sync::RwLock<Option<...>>`:

```rust
static MCP_SERVICE: StdRwLock<Option<Arc<RwLock<McpService>>>> = ...;
static SEARCH_CONFIG: StdRwLock<Option<SearchConfig>> = ...;
```

- `install_mcp_service(svc)` / `mcp_service()` — the shared MCP service
- `install_search_config(cfg)` / `search_config()` — the resolved config
- `reset_for_tests()` — clears both slots (test-only)

Slots are populated once at startup. Production code treats them as
immutable after bootstrap. Tests can override them between cases.

### Bootstrap (`bootstrap.rs`)

`bootstrap_search_backend(config)` is called from `main.rs`,
`tui/mod.rs`, `exec.rs`, and `core/daemon.rs`. It:

1. Returns the existing service if already installed (idempotent).
2. Calls `bootstrap_eggsearch(config)` which:
   - Resolves the effective `SearchConfig` from `config.search`.
   - Installs it into the state slot.
   - Skips MCP setup unless `backend = "eggsearch"`.
   - If an explicit `[mcp.eggsearch]` block exists, uses
     `connect_from_config` to honor it.
   - Otherwise spawns `eggsearch` via `McpService::connect_stdio`.
   - Lists advertised tools and classifies coverage.
3. Calls `provider_status` (best-effort, never breaks startup).
4. Records required/recommended tool coverage.

Returns `(Option<Arc<RwLock<McpService>>>, BootstrapReport)`.

### Tool Coverage Classification

**Required tools** (`EGGSEARCH_REQUIRED_TOOLS`):
- `web_search`, `web_fetch`

If any required tool is missing, coverage is `"incompatible"`.

**Recommended tools** (`EGGSEARCH_RECOMMENDED_TOOLS`):
- `batch_fetch`, `repo_search`, `repo_fetch`, `repo_map`,
  `security_search`, `research_search`, `build_evidence_bundle`

If all required are present but some recommended are missing,
coverage is `"partial"`. If all are present, `"complete"`.

### Dispatch (`mod.rs`)

Nine dispatch functions resolve the current `SearchConfig`, then
delegate to the eggsearch adapter or legacy fallback:

| Function | Backend support | Fallback |
|----------|----------------|----------|
| `dispatch_web_search` | eggsearch / builtin | Yes (if `fallback_to_builtin`) |
| `dispatch_web_fetch` | eggsearch / builtin | Yes (if `fallback_to_builtin`) |
| `dispatch_repo_search` | eggsearch only | No |
| `dispatch_repo_fetch` | eggsearch only | No |
| `dispatch_repo_map` | eggsearch only | No |
| `dispatch_security_search` | eggsearch only | No |
| `dispatch_research_search` | eggsearch only | No |
| `dispatch_batch_fetch` | eggsearch only | No |
| `dispatch_evidence_bundle` | eggsearch only | No |

Each has a `dispatch_*_structured` variant that returns
`StructuredSearchResult` (output + optional structured value +
truncated flag). The structured variants are used by CodeGG's native
tool wrappers via `Tool::execute_structured()`.

### Eggsearch Adapter (`eggsearch.rs`)

Translates native tool arguments to eggsearch MCP tool arguments.
Key behaviors:

- **web_search**: Reads `query` (required), `num_results`/`max_results`
  (default 8, cap 30), `provider` hint → `providers` list, plus
  `intent`, `freshness`, `safe_search`.
- **web_fetch**: Reads `url` (required, validated), `max_length`/
  `max_chars` (default 10k), `extract_mode`, `include_links`.
- **repo_search**: Reads `query` (required), optional repo locator
  (`owner`+`repo` or combined `owner/repo`), forwards `host`, `path`,
  `file`, `language`, `symbol`, `profile`, `include_local`, `mode`.
  Rejects `include_snippets`.
- **repo_fetch**: Requires `path` + repo locator. Forwards `commit_sha`,
  line ranges (`line_start`/`line_end` with `start_line`/`end_line`
  aliases), context, symbol, block-expansion, local-preference.
- **repo_map**: Requires repo locator. Forwards `max_depth` (default 2,
  cap 3). Rejects subdirectory `path` (use `repo_search` or
  `repo_fetch`).
- **security_search**: Reads `query`, maps legacy `cve` → `cve_id`,
  forwards GHSA/OSV/RustSec, package/version, applicability, workflow.
- **research_search**: Reads `query`, forwards `research_domain`,
  `desired_source_types`, workflow/depth, freshness, providers,
  comparison, constraints, context. Legacy `domains` array translated
  only when unambiguous.
- **batch_fetch**: Canonicalizes `urls` and `items` into tagged
  `{type: "web"|"repo", ...}` items. Validates all web URLs.
- **build_evidence_bundle**: Accepts `sources` and/or `fetches`.
  Rejects historical `type` field on sources.

Each adapter calls `ensure_tool_available()` before dispatch, producing
an actionable error if the upstream MCP tool is missing.

### Legacy Adapter (`legacy.rs`)

Uses `SearchProviderRegistry::from_env()` to pick a provider from
environment variables. Returns a formatted hit list. Errors with a
clear message when no providers are configured. No structured variant.

### Trust Framing (`framing.rs`)

Every eggsearch result is wrapped in `external_untrusted` framing:

```text
[external_web_content trust=external_untrusted source=eggsearch tool=websearch]
[external_repo_evidence trust=external_untrusted source=eggsearch tool=repo_search]
[external_security_evidence trust=external_untrusted source=eggsearch tool=security_search]
[external_research_evidence trust=external_untrusted source=eggsearch tool=research_search]
[external_evidence_bundle trust=external_untrusted source=eggsearch tool=build_evidence_bundle]
```

Fetch and evidence frames include a stronger warning about attacker-
controlled content. The `source` parameter is configurable so builtin
backend framing does not claim `source=eggsearch`.

Output is clamped by `clamp_output()` which truncates at a byte
boundary and appends `[truncated by Codegg: ...]`. Returns
`(output, truncated)`.

### Structured Response Contract

Eggsearch wrappers use the `dispatch_*_structured` path when
`Tool::execute_structured()` is called. The MCP client retains JSON
content from `structuredContent` or `content[type=json]`. When
eggsearch sends serialized JSON in a text content part, the adapter
parses that complete text before applying any output cap. The resulting
`serde_json::Value` is stored in `StructuredToolResult::value` while
`StructuredToolResult::output` remains the bounded, trust-framed
projection.

Display truncation cannot corrupt stable IDs, trust markers, routing
decisions, or domain metadata. Unknown additive fields are retained.
A text-only response from an older server yields `value = None`.

### Provenance

Each dispatch function has a corresponding `provenance_for_*` helper
that builds a `ToolProvenance` describing the backend, implementation,
and trust level. Used by the agent loop for audit trails.

### Hiding Raw MCP Tools

The agent loop's `build_tool_definitions` filters out tools whose
name starts with `mcp__<server_name>__` when
`expose_raw_mcp_tools = false` (the default). The server name is
resolved from `SearchConfig` so custom names are honored.

## Key Types & APIs

| Type | File:Line | Purpose |
|------|-----------|---------|
| `SearchConfig` | `config/src/schema.rs:463` | Backend, output caps, eggsearch config |
| `SearchBackendConfig` | `config/src/schema.rs:557` | Eggsearch / Builtin / Disabled |
| `EggsearchConfig` | `config/src/schema.rs:568` | Server name, command, args, timeouts |
| `ToolTimeoutKind` | `config/src/schema.rs:584` | Default / Security / Research / Batch |
| `McpService` | `src/mcp/mod.rs:109` | MCP server registry (consumed by eggsearch adapter) |
| `StructuredSearchResult` | `src/search_backend/mod.rs:47` | output + value + truncated |
| `EggsearchCallResult` | `eggsearch.rs:389` | output, value, truncated (per-call) |
| `BootstrapReport` | `src/search_backend/bootstrap.rs:275` | Startup diagnostic report |
| `CrossProcessLockGuard` | `src/search_backend/test_support.rs:23` | Test isolation via flock |

## Configuration Surface

```toml
[search]
backend = "eggsearch"           # "eggsearch" | "builtin" | "disabled"
expose_raw_mcp_tools = false
fallback_to_builtin = false
max_search_output_chars = 12000
max_fetch_output_chars = 20000
max_repo_output_chars = 15000
max_repo_search_output_chars = 15000   # falls back to max_repo_output_chars
max_repo_fetch_output_chars = 15000    # falls back to max_repo_output_chars
max_repo_map_output_chars = 15000      # falls back to max_repo_output_chars
max_security_output_chars = 10000
max_research_output_chars = 15000
max_batch_output_chars = 50000
max_evidence_output_chars = 100000

[search.eggsearch]
enabled = true
server_name = "eggsearch"
command = "eggsearch"
args = ["mcp", "stdio"]
timeout_ms = 60000                    # default call timeout
repo_timeout_ms = 60000               # optional per-domain overrides
security_timeout_ms = 60000
research_timeout_ms = 60000
batch_fetch_timeout_ms = 60000
provider_status_timeout_ms = 15000    # health check (shorter)

[search.eggsearch.env]
BRAVE_SEARCH_API_KEY = "$BRAVE_SEARCH_API_KEY"
```

Defaults: `backend = "eggsearch"`, `server_name = "eggsearch"`,
`command = "eggsearch"`, `args = ["mcp", "stdio"]`.

## Invariants & Gotchas

- **websearch/webfetch always registered** — they fall back to error
  messages or builtin implementations when eggsearch is unavailable.
- **Seven expanded wrappers conditionally registered** — based on
  `evidence_config.enabled`. When `[search].backend = "disabled"`,
  `EvidenceBackendRuntimeConfig.enabled` is `false` and the wrappers
  are omitted from the tool registry entirely.
- **Only websearch/webfetch support builtin fallback** — the seven
  expanded wrappers (`repo_search`, etc.) require `backend = "eggsearch"`
  and return an error otherwise.
- **Reentrant bootstrap is safe** — `bootstrap_search_backend` checks
  `state::mcp_service().is_some()` before re-connecting.
- **Output caps are byte-based** — `clamp_output` operates on byte
  length. UTF-8 boundary issues are vanishingly rare for ASCII-heavy
  web output.
- **ensure_tool_available is best-effort during bootstrap** — when the
  `McpService` write lock is held (e.g., during bootstrap), the tool
  availability check returns `Ok(())` via the `try_read` WouldBlock
  path. The actual call will surface the real error if the tool is
  missing.
- **Provider hint translation is permissive** — unknown historical
  provider hints map to an empty list (auto-pick) so the search
  still succeeds with a sensible default.
- **Repository locator disambiguation** — `repo = "a/b"` splits to
  `owner=a, repo=b`. `repo = "a/b/c"` is ambiguous and rejected.
  Providing both `owner` and a `/`-containing `repo` is an error.
- **Batch item routing precedence** — items with `type=web` or no
  `type` + a `url` key route to web fetch. Only when web does not
  match does the repo branch attempt to parse `repo`/`path`.

## Doctor

```bash
codegg doctor search
```

Output is `BootstrapReport::summary_lines()` covering: backend, server
name, command, MCP connection status, advertised tools, tool coverage
classification (complete/partial/incompatible with missing tool lists),
required/recommended coverage, default timeout, provider status,
`expose_raw_mcp_tools`, `fallback_to_builtin`, and all per-domain
output caps.

## Where to Add New Providers

New web search providers belong in the **eggsearch** project, not in
Codegg's built-in search provider registry (`src/search/`). The built-
in registry is legacy fallback only. Codegg owns the wrapper UX,
permissioning, output caps, trust framing, and backend selection; the
actual search/fetch logic lives in eggsearch.

## Testing

```bash
# Narrowest: single dispatch unit tests
cargo test -p codegg search_backend::tests

# Eggsearch adapter tests
cargo test -p codegg search_backend::eggsearch::tests

# Bootstrap tests
cargo test -p codegg search_backend::bootstrap::tests

# Framing tests
cargo test -p codegg search_backend::framing::tests

# End-to-end with mock MCP (no real eggsearch binary)
cargo test -p codegg --test fake_eggsearch_mcp

# Argument mapping tests
cargo test -p codegg --test search_backend_arg_mapping

# Legacy adapter tests
cargo test -p codegg --test search_backend_legacy

# Eggsearch integration tests
cargo test -p codegg --test search_backend_eggsearch

# Compatibility smoke test (opt-in, needs installed eggsearch)
CODEGG_EGGSEARCH_BIN=/path/to/eggsearch \
  cargo test -p codegg --test eggsearch_real_compat -- --ignored --nocapture
```

`fake_eggsearch_mcp` exercises the full dispatch path using an in-
process mock `McpService` via `register_mock_server`.

## Related Docs

- [mcp.md](mcp.md) — `McpService` plumbing that eggsearch consumes
- [tool.md](tool.md) — the `websearch`, `webfetch`, and eggsearch
  wrapper tools
- [config.md](config.md) — config loading and validation
- [security.md](security.md) — SSRF protection, trust framing
