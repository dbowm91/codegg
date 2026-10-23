# Search Backend Module

Wrapper layer between CodeGG's agent-facing search/evidence tools and
the external `eggsearch` MCP server. Search execution is either
Eggsearch-backed or disabled; CodeGG has no second external provider
implementation or semantic fallback.

## Purpose

Codegg exposes stable native tool names (`websearch`, `webfetch`,
`repo_search`, `repo_fetch`, `repo_map`, `security_search`,
`research_search`, `batch_fetch`, `evidence_bundle`) to the agent.
Internally they delegate to the configured Eggsearch MCP backend or
return a disabled/unavailable error. The search backend
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
├── mod.rs           # SearchRuntimeContext methods (canonical) + legacy global wrappers
├── context.rs       # Explicit runtime-owned SearchRuntimeContext (M005)
├── state.rs         # Deprecated process-global slots (bootstrap reuse + legacy compat only)
├── bootstrap.rs     # Connect eggsearch at startup; emit BootstrapReport
├── eggsearch.rs     # Adapter: native args → eggsearch MCP args (explicit McpHandle)
├── framing.rs       # external_untrusted framing + clamp_output
└── test_support.rs  # Cross-process flock (bootstrap compat tests only)
```

## How It Works

### Runtime Context (`context.rs`) — canonical (M005)

`SearchRuntimeContext` is the explicit runtime-owned carrier for search
execution. It holds an owned immutable `SearchConfig` snapshot plus the
shared daemon-owned `McpService` handle (`Option<Arc<RwLock<McpService>>>`):

- constructed after config/MCP bootstrap via
  `bootstrap_search_runtime()`, or via `SearchRuntimeContext::new/disabled`
  for tests and bootstrap-free paths;
- threaded through `ToolRegistryOptions::search_runtime` into every
  search/evidence wrapper and stored on the registry
  (`ToolRegistry::search_runtime()`);
- cloned by `Arc` only for the actually shared MCP transport; dropping
  one context never tears down the shared service;
- immutable after construction — no install/reset API, so runtime
  restart reconstructs from config/bootstrap and no stale value leaks;
- `Debug` redacts `[search.eggsearch.env]` values (keys only), and the
  context is never serialized wholesale.

The `dispatch_*`/`provenance_for_*` methods on the context are the
single canonical execution path. Two independently constructed contexts
with different configs coexist in one process without cross-talk
(proven by `tests/config_provider/search_runtime_isolation.inc`).

Ownership split: the `SearchConfig` snapshot is per-runtime (each turn
may resolve its own), while the MCP transport remains the one
daemon-owned shared service reused across entry points (so bootstrap
does not spawn one eggsearch server process per turn). Concurrent tool
calls share the daemon service's existing synchronization; no extra
global lookup lock is taken per call.

The runtime context does not carry a session workspace root. In pinned
Eggsearch v0.4.4, `repo_search.include_local` selects local search against
the sidecar's configured `[local].roots`; it does not bind those roots to
the active CodeGG session. CodeGG must not describe such a result as
attached-workspace coverage unless the configured roots have been verified
for that workspace. Per-session workspace binding needs an explicit
upstream request contract or session-scoped sidecar lifecycle; the current
adapter does not synthesize a root from a model argument.

### Legacy State (`state.rs`) — deprecated compatibility

Two process-global slots backed by `std::sync::RwLock<Option<...>>`
remain for narrow compatibility only:

```rust
static MCP_SERVICE: StdRwLock<Option<Arc<RwLock<McpService>>>> = ...;
static SEARCH_CONFIG: StdRwLock<Option<SearchConfig>> = ...;
```

- `install_mcp_service(svc)` / `mcp_service()` — daemon connection
  reuse across process entry points (bootstrap only);
- `install_search_config(cfg)` / `search_config()` — legacy global
  dispatch wrappers snapshot (never mutate) these;
- `reset_for_tests()` — bootstrap compat tests only.

Production tool execution must not call these. New code takes a
`SearchRuntimeContext`. The free `dispatch_*`/`provenance_for_*` functions in
`mod.rs` are thin legacy wrappers over a global snapshot
(`context::snapshot_global()`) for backward-compatible callers
(deep-research fallback, diagnostics, and tests that explicitly exercise the
legacy global path). `SearchRuntimeContext::install_as_global_compat`
(`src/search_backend/context.rs:154`) is the only sanctioned writer besides
bootstrap — and it currently has **no caller**, so in production the global
slots are written by bootstrap alone.

### Bootstrap (`bootstrap.rs`)

`bootstrap_search_runtime(config)` is the production entry point,
called from turn construction (`agent/turn_runtime.rs`), subagent spawn
(`agent/worker.rs`), `exec.rs`, and single-shot `main.rs` flows. It
resolves the effective config, reuses/establishes the shared daemon MCP
transport via `bootstrap_search_backend`, and returns an explicit
`(SearchRuntimeContext, BootstrapReport)` — the context flows into tool
registry construction so no wrapper reads globals at execution time.

`bootstrap_search_backend(config)` (legacy, retained for diagnostics and
connection reuse) is called from `main.rs` doctor (`src/main.rs:3078`) and
`tui` diagnostics (`src/tui/commands/diagnostics.rs:209`, `:270`). It is
**not** called from `core/daemon.rs` — that file contains no search-backend
reference at all. It:

1. Returns the existing service if already installed (idempotent).
2. Calls `bootstrap_eggsearch(config)` which:
   - Resolves the effective `SearchConfig` from `config.search`.
   - Installs it into the state slot.
   - Skips MCP setup unless `backend = "eggsearch"`.
   - If an explicit `[mcp.eggsearch]` block exists, uses
     `connect_from_config` to honor it (authoritative).
   - Otherwise resolves the spawn command via `crate::install`:
     explicit `[search.eggsearch].command` wins when set (advanced
     override, distinguished via `has_explicit_command()`); else the
     installation-owned `codegg-eggsearch` sibling wins; else legacy
     PATH `eggsearch` remains as source-build compatibility only.
     `cwd` and helper-specific env vars are never authoritative.
   - Probes managed-sidecar presence/version (best-effort, never fatal)
     and records `resolution_source` plus an installation-specific hint
     (reinstall the bundle, never bare "install eggsearch" as the lead).
   - Lists advertised tools and classifies coverage.
3. Calls `provider_status` (best-effort, never breaks startup).
4. Records required/recommended tool coverage plus the ten-tool bundled
   surface (`EGGSEARCH_BUNDLED_COMPLETE_SURFACE`, including
   `provider_status`); MCP discovery stays authoritative.

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
delegate to the eggsearch adapter or return an error when disabled or
unavailable:

| Function | Backend support |
|----------|-----------------|
| `dispatch_web_search` | eggsearch |
| `dispatch_web_fetch` | eggsearch |
| `dispatch_repo_search` | eggsearch |
| `dispatch_repo_fetch` | eggsearch |
| `dispatch_repo_map` | eggsearch |
| `dispatch_security_search` | eggsearch |
| `dispatch_research_search` | eggsearch |
| `dispatch_batch_fetch` | eggsearch |
| `dispatch_evidence_bundle` | eggsearch |

These exist twice over:

- as **methods on `SearchRuntimeContext`** (`src/search_backend/mod.rs:123`),
  which is the canonical execution path. The plain variants return
  `Result<String, ToolError>` — the framed, capped model projection;
- as **free legacy wrappers** generated by the `legacy_dispatch!` macro
  (`src/search_backend/mod.rs:853-861`), which snapshot the deprecated
  global slots and delegate to the same methods.

Each of the nine also has a `dispatch_*_structured` variant that returns
`StructuredSearchResult` (output + value + truncated + success). Six are
generated by the `structured_eggsearch_dispatch!` macro
(`src/search_backend/mod.rs:601-648`); `web_search`, `web_fetch`, and
`evidence_bundle` are hand-written. The structured variants are used by
CodeGG's native tool wrappers via `Tool::execute_structured()`.

Three repo methods have an additional **host-scoped** form —
`dispatch_repo_search_host_scoped` (`:127`),
`dispatch_repo_fetch_host_scoped` (`:162`), and
`dispatch_repo_map_host_scoped` (`:197`). Each takes the
daemon-authorized workspace root (`Option<&Path>`) and returns a
`StructuredSearchResult`; `None` explicitly disables local retrieval on the
managed child rather than letting it fall back to static roots.

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

### Trust Framing (`framing.rs`)

Every eggsearch result is wrapped in `external_untrusted` framing. The
frame kind follows the tool, not the backend:

```text
[external_web_content trust=external_untrusted source=eggsearch tool=websearch]
[external_web_content trust=external_untrusted source=eggsearch tool=webfetch]
[external_web_content trust=external_untrusted source=eggsearch tool=batch_fetch]
[external_repo_evidence trust=external_untrusted source=eggsearch tool=repo_search]
[external_repo_evidence trust=external_untrusted source=eggsearch tool=repo_fetch]
[external_repo_evidence trust=external_untrusted source=eggsearch tool=repo_map]
[external_security_evidence trust=external_untrusted source=eggsearch tool=security_search]
[external_research_evidence trust=external_untrusted source=eggsearch tool=research_search]
[external_evidence_bundle trust=external_untrusted source=eggsearch tool=build_evidence_bundle]
```

Each frame is closed with a matching `[/<frame-kind>]` line. Fetch and
evidence frames include a stronger warning about attacker-controlled
content. External results always identify Eggsearch as their source.

**The frame is a prompt-injection defence, and it belongs to the model path.**
The model receives it byte for byte. The TUI *parses* the frame **for display
only**: the expanded detail view shows the payload inside the frame instead of
the banner, and the banner is never presented to the user as the tool's result
(`architecture/tui.md#the-detail-view-the-payload-not-the-banner`). That line
does not weaken the frame — `src/search_backend/framing.rs` is unchanged, the
extraction is a bounded read that never rewrites what the model receives, and
the TUI duplicates the marker names because the framing module is the model
path's contract and is not the TUI's to change.

Output is clamped by `clamp_output(content, max_chars, label)`
(`src/search_backend/framing.rs:163`), which truncates to the longest
UTF-8-valid prefix within the byte cap
(`truncate_utf8_boundary`, `:183`) and appends
`[truncated by Codegg: output exceeded <label>=<max_chars>]`. Returns
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

### Programmatic (Tool Program) availability

Only `repo_search` is callable from Tool Programs (expansion M002; see
`architecture/tool_programs.md#expansion-m002`). Its tool contract is
`DirectOrProgrammatic` / `ReadOnly` / `Idempotent` with no broker retry
and the program-call cache disabled: repeated identical queries
re-execute against the backend, ledger replay serves the recorded
execution-time result, and a fresh rerun may return different results.
Every other search/fetch wrapper (`websearch`, `webfetch`,
`repo_fetch`, `repo_map`, `codesearch`, `batch_fetch`,
`security_search`, `research_search`, `evidence_bundle`) remains
`DirectOnly`. Programs supply only query/filters through the existing
broker pipeline; provider credentials stay in the daemon-owned
`SearchRuntimeContext`.

### Hiding Raw MCP Tools

The agent loop's tool-definition builder
(`src/agent/request_preparation.rs:922-966`) builds an
`McpExposurePolicy { show_raw: true, hidden_servers }` and calls
`McpService::list_filtered_tools` (`src/mcp/mod.rs:681`). Raw
`mcp__<server>__<tool>` definitions for a **hidden server** are dropped
whole — the filter is by server name, not by a name prefix inside the
builder.

- The eggsearch server name is resolved from `SearchConfig`
  (`search.eggsearch.server_name`, default `eggsearch`), so custom names
  are honored.
- `[search].expose_raw_mcp_tools = true` (default `false`) unhides the
  eggsearch server's raw tools.
- The same policy covers the per-domain `[tool_backends.*]` backends
  (lsp, security, context): a domain that sets
  `expose_raw_mcp_tools = true` is removed from `hidden_servers`, and a
  domain that does not is added to it. This is the forward-compatible hook
  for the future `egglsp`/`eggsentry` MCP adapters.
- User-configured third-party MCP servers are **not** hidden — only the
  CodeGG-managed backends are.

## Key Types & APIs

| Type | File:Line | Purpose |
|------|-----------|---------|
| `SearchConfig` | `crates/codegg-config/src/schema.rs:1120` | Backend, output caps, eggsearch config |
| `SearchBackendConfig` | `crates/codegg-config/src/schema.rs:1207` | Eggsearch / Disabled |
| `EggsearchConfig` | `crates/codegg-config/src/schema.rs:1235` | Server name, command, args, timeouts |
| `ToolTimeoutKind` | `crates/codegg-config/src/schema.rs:1251` | Default / Security / Research / BatchFetch / ProviderStatus |
| `McpService` | `src/mcp/mod.rs:165` | MCP server registry (consumed by eggsearch adapter) |
| `StructuredSearchResult` | `src/search_backend/mod.rs:54` | output + value + truncated + success |
| `EggsearchCallResult` | `src/search_backend/eggsearch.rs:587` | output, value, truncated (per-call) |
| `BootstrapReport` | `src/search_backend/bootstrap.rs:453` | Startup diagnostic report |
| `CrossProcessLockGuard` | `src/search_backend/test_support.rs:25` | Test isolation via flock |

## Configuration Surface

```toml
[search]
backend = "eggsearch"           # "eggsearch" | "disabled"
expose_raw_mcp_tools = false
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
`args = ["mcp", "stdio"]`. An absent `[search.eggsearch].command` resolves
the installation-owned `codegg-eggsearch` sibling (see `src/install.rs`);
only an explicit `command` overrides it. Legacy PATH `eggsearch` is
source-build compatibility, never required for a prebuilt install.

## Invariants & Gotchas

### Host-scoped local repository retrieval

The CodeGG-managed Eggsearch stdio child receives a fresh process secret at
startup. `repo_search`, `repo_fetch`, and `repo_map` calls carry a private MCP
request metadata scope containing the accepted invocation's canonical
workspace root, or `root: null` when the broker context cannot establish that
scope. The secret authenticates this transport extension; it is never part of
the model tool schema. Eggsearch 0.4.4+ rejects unauthenticated/malformed
metadata and never falls back to static `[local].roots` when host metadata is
present. Explicit remote MCP Eggsearch is not eligible for host-scoped local
retrieval. The bounded Eggsearch cache is per canonical root and indexing
configuration; it contains no authorization state. CodeGG remains the sole
authority for selecting a root on each invocation.

- **websearch/webfetch always registered** — unavailable Eggsearch returns
  an actionable error; it never invokes another provider implementation.
- **Seven expanded wrappers conditionally registered** — on
  `evidence_config.enabled` **and** `evidence_config.backend == "eggsearch"`
  (`src/tool/mod.rs:791-794`). When either fails — including
  `[search].backend = "disabled"`, where `EvidenceBackendRuntimeConfig.enabled`
  is `false` — the wrappers are omitted from the tool registry entirely and
  a diagnostic is logged.
- **No semantic fallback** — Eggsearch errors remain visible to the caller;
  request and provider semantics are never changed by CodeGG.
- **Removed configuration fails actionably** — `backend = "builtin"` directs
  users to restore/configure the managed or explicit Eggsearch sidecar, or
  select `disabled`. `fallback_to_builtin` is rejected and must be removed.
- **Reentrant bootstrap is safe** — `bootstrap_search_backend` checks
  the installed service before re-connecting, so one daemon eggsearch
  connection is shared instead of spawned per turn.
- **Output caps are byte-based** — `clamp_output` operates on byte
  length. UTF-8 boundary issues are vanishingly rare for ASCII-heavy
  web output.
- **ensure_tool_available is best-effort during bootstrap** — when the
  `McpService` write lock is held (e.g., during bootstrap), the tool
  availability check returns `Ok(())` via the `try_read` WouldBlock
  path. The actual call will surface the real error if the tool is
  missing.
- **Provider hints fail closed** — absent/`auto` omits the provider selector;
  each other exposed value maps to a provider ID in eggsearch v0.4.4 or fails
  before MCP dispatch. `github` maps to `github_repositories`; `brave` maps
  to the distinct `brave` provider; retired `google_news` gives guidance to
  use `intent="news"` with `brave_api` or `tavily` when source control matters.
- **Provider health is separate from contract compatibility** — `doctor
  search` reports an eggsearch version mismatch or missing wrapper provider ID
  as compatibility defects, and reports disabled, missing-credential, and
  cooldown/degraded providers as operational state.
- **Repository locator disambiguation** — `repo = "a/b"` splits to
  `owner=a, repo=b`. `repo = "a/b/c"` is ambiguous and rejected.
  Providing both `owner` and a `/`-containing `repo` is an error.
- **Batch item routing precedence** — items with `type=web` or no
  `type` + a `url` key route to web fetch. Only when web does not
  match does the repo branch attempt to parse `repo`/`path`.

## Doctor

```bash
codegg doctor search
codegg doctor installation
```

Output is `BootstrapReport::summary_lines()` covering: backend,
resolution source (`explicit-mcp`/`explicit-command`/`managed-sidecar`/
`legacy-path`), server name, command, managed-sidecar presence/version,
MCP connection status, advertised tools, tool coverage classification
(complete/partial/incompatible with missing tool lists), ten-tool bundled
surface, required/recommended coverage, default timeout, provider status,
installation hint (bundle reinstall, never bare "install eggsearch"),
`expose_raw_mcp_tools`, and all per-domain output caps. The search doctor
checks exact eggsearch pin `0.4.4`, its ten-tool
surface, and every provider ID retained by the native wrapper. `doctor
installation` prints `InstallationReport` (executable,
directory, sibling states, helper/kernel probe, in-process eggsact line)
and points to `doctor search` for tool coverage.

### Development paired compatibility gate

Run the development-only offline pair gate against an explicitly selected
binary:

```bash
CODEGG_EGGSEARCH_BIN=/absolute/path/to/eggsearch \
  scripts/test-eggsearch-pair.sh
```

Or build from the exact immutable source tag into a temporary target directory:

```bash
CODEGG_EGGSEARCH_SOURCE=/path/to/eggsearch \
  scripts/test-eggsearch-pair.sh
```

The harness checks the exact version and SHA-256 identity, MCP initialize,
the ten-tool inventory, `provider_status(probe=false)`, retained provider IDs,
fake-MCP structured evidence, wrapper argument mapping, negative selector
validation, and child shutdown in one command. It does not run provider
requests or CodeGG release tooling. The subprocess runs with credentials
cleared, using a temporary HOME/config/workspace.

## Where to Add New Providers

New web search providers belong in the **eggsearch** project. CodeGG owns
the wrapper UX, permissioning, output caps, trust framing, and backend
selection; the actual search/fetch logic lives in eggsearch.

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

# End-to-end with mock MCP (no real eggsearch binary).
# These are members of the `config_provider` test family, so the
# family/member form is required.
cargo test --test config_provider -- fake_eggsearch_mcp

# Argument mapping tests
cargo test --test config_provider -- search_backend_arg_mapping

# Eggsearch integration tests
cargo test --test config_provider -- search_backend_eggsearch

# Context isolation
cargo test --test config_provider -- search_runtime_isolation

# Compatibility smoke test (opt-in, needs installed eggsearch)
CODEGG_EGGSEARCH_BIN=/path/to/eggsearch \
  cargo test --test config_provider -- eggsearch_real_compat -- --ignored --nocapture
```

`fake_eggsearch_mcp` exercises the full dispatch path using an in-
process mock `McpService` via `register_mock_server`, with each test
owning an explicit `SearchRuntimeContext` (no globals, no locks).
`tests/config_provider/search_runtime_isolation.inc` proves two differently configured
contexts/registries coexist concurrently without cross-talk, disabled
stays disabled, and reconstruction leaks no stale config. The
cross-process flock in `test_support.rs` is retained only for the
bootstrap compat tests that still assert on the legacy install slots.

## Related Docs

- [mcp.md](mcp.md) — `McpService` plumbing that eggsearch consumes
- [tool.md](tool.md) — the `websearch`, `webfetch`, and eggsearch
  wrapper tools
- [config.md](config.md) — config loading and validation
- [security.md](security.md) — SSRF protection, trust framing

## Current source verification

Verified after legacy-retirement M002 against `src/search_backend/`,
`src/tool/webfetch.rs`, `src/tool/websearch.rs`, and
`crates/codegg-config/src/schema.rs`. `SearchBackendConfig` has only
`Eggsearch` and `Disabled`; legacy external-provider and webfetch HTTP
execution modules have been deleted. The old `backend = "builtin"` and
`fallback_to_builtin` settings fail during config deserialization with
migration guidance.

Verified 2026-10-23 against `src/search_backend/` (all seven modules),
`crates/codegg-config/src/schema.rs`, `src/mcp/mod.rs`,
`src/mcp/protocol.rs`, `src/install.rs`, `src/tool/mod.rs`,
`src/tool/integrated_config.rs`, `src/tool/codesearch.rs`,
`src/agent/request_preparation.rs`, `src/agent/turn_runtime.rs`,
`src/agent/worker.rs`, `src/exec.rs`, `src/main.rs`,
`src/tui/commands/diagnostics.rs`, `tests/config_provider/`, and
`scripts/test-eggsearch-pair.sh`. Corrections made:

| Claim in the previous revision | Finding |
|---|---|
| `SearchConfig` at `schema.rs:790`, `SearchBackendConfig` `:877`, `EggsearchConfig` `:905`, `ToolTimeoutKind` `:921` | All four were stale. Corrected to `:1120`, `:1207`, `:1235`, `:1251`. The other `src/…` line references were re-checked and are correct. |
| `StructuredSearchResult` — "output + value + truncated" | It has a fourth field, `success: bool` (`src/search_backend/mod.rs:54`). `into_tool_result` (`mod.rs:61`) forwards it to `StructuredToolResult::with_value` / `with_provenance`. |
| `ToolTimeoutKind` — "Default / Security / Research / Batch" | Five variants: the fifth is `ProviderStatus` (`schema.rs:1251`). |
| "`bootstrap_search_backend` … is called from … `core/daemon.rs` paths" | Dead reference. `src/core/daemon.rs` contains no search-backend reference. The real call sites are `src/main.rs:3078` (doctor) and `src/tui/commands/diagnostics.rs:209`, `:270`. |
| "The free `dispatch_*`/`provenance_for_*` functions in `mod.rs` are thin legacy wrappers over a global snapshot" | Correct, and now precise about the mechanism: they are generated by the `legacy_dispatch!` / `legacy_dispatch_structured!` / `legacy_provenance!` macros at `mod.rs:853-879` and delegate to `context::snapshot_global()` (`context.rs:167`). `install_as_global_compat` (`context.rs:154`) has **no caller**. |
| The five trust-frame markers listed under "Trust Framing" | Incomplete. There are nine: `external_web_content` covers `websearch`, `webfetch` **and** `batch_fetch`; `external_repo_evidence` covers `repo_search`, `repo_fetch` **and** `repo_map`. Each frame is closed with a matching `[/<kind>]` line. |
| "Output is clamped by `clamp_output()` which truncates at a byte boundary" | Now precise: `clamp_output(content, max_chars, label)` (`framing.rs:163`) truncates to the longest UTF-8-valid prefix within the byte cap (`truncate_utf8_boundary`, `:183`) and appends `[truncated by Codegg: output exceeded <label>=<max_chars>]`. |
| "Seven expanded wrappers conditionally registered — based on `evidence_config.enabled`" | Two conditions, not one: `evidence_enabled && evidence_is_eggsearch` (`src/tool/mod.rs:791-794`). |
| "The agent loop's `build_tool_definitions` filters out tools whose name starts with `mcp__<server_name>__`" | The mechanism is server-level hiding: `McpExposurePolicy { show_raw: true, hidden_servers }` built at `src/agent/request_preparation.rs:922-966` and applied by `McpService::list_filtered_tools` (`src/mcp/mod.rs:681`). Third-party MCP servers stay visible. |
| `cargo test -p codegg --test fake_eggsearch_mcp` (and the three sibling commands) | Those targets no longer exist as standalone integration tests. All of them are members of the `config_provider` **family** (`tests/config_provider/*.inc`), so the family/member form `cargo test --test config_provider -- <member>` is required. |
| — | Added: the three `dispatch_*_host_scoped` repo methods and the `Option<&Path>` root contract; the fact that all nine plain dispatches return `Result<String, ToolError>` while the structured ones return `StructuredSearchResult`; the `McpExposurePolicy` per-domain `[tool_backends.*]` rule; the host-scope version gate `>=0.4.4, <0.5.0` (`src/mcp/mod.rs:171`), which is the concrete form of the "Eggsearch 0.4.4+" claim; and the `search_runtime_isolation` family member. |

Confirmed still accurate: the module tree and its file responsibilities;
`EGGSEARCH_REQUIRED_TOOLS` (`bootstrap.rs:22`), `EGGSEARCH_RECOMMENDED_TOOLS`
(`:24`), and the ten-tool `EGGSEARCH_BUNDLED_COMPLETE_SURFACE` (`:38`);
the `SearchBackendConfig` variants and the `"builtin"` migration error
(`schema.rs:1207-1231`); the `codesearch` compatibility alias over
`repo_search` with `profile = "coding"` (`src/tool/codesearch.rs:68`); every
config key and default in the "Configuration Surface" section (including
`max_*_output_chars` defaults and the `max_repo_*` fallbacks, `args =
["mcp","stdio"]`, `timeout_ms = 60000`, `provider_status_timeout_ms =
15000`); the pinned eggsearch version `0.4.4` and commit
`32f48bb57a43274d763c7f9ba919eab84d125edc` (`src/install.rs:22`, `:24` — the
installation-owned pin; `scripts/test-eggsearch-pair.sh:6` pins a *different*
`0.4.4` commit, `13fcac0b…` — see the divergence recorded in
[mcp.md](mcp.md));
the `repo_search`-only Tool Program contract
(`src/tool/repo_search.rs:170`); and the doctor command names.
