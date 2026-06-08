# Codegg Eggsearch Native Wrapper: Final Tightening Plan

## Purpose

Codegg now has the correct high-level architecture for search and fetch:

- The agent-facing tools remain the native `websearch` and `webfetch` tools.
- Those tools dispatch through `src/search_backend`.
- `eggsearch` is intended to be the default backend.
- The legacy in-tree search/fetch implementation is retained only as explicit fallback or `backend = "builtin"`.
- Raw `mcp__eggsearch__*` tools are hidden from the model by default.
- Eggsearch outputs are wrapped as `external_untrusted` before being returned to the model.

This final pass should not redesign the integration. It should tighten the remaining correctness gaps so the implementation behaves predictably in normal Codegg usage.

## Current state summary

The second implementation pass added the right modules and control flow:

- `src/search_backend/mod.rs` defines `dispatch_web_search` and `dispatch_web_fetch`.
- `src/search_backend/eggsearch.rs` translates Codegg-native arguments into eggsearch MCP arguments.
- `src/search_backend/bootstrap.rs` creates/connects an `McpService` for eggsearch and installs shared state.
- `src/search_backend/state.rs` stores process-wide search config and MCP service handles.
- `src/search_backend/framing.rs` wraps search/fetch outputs with `external_untrusted` framing.
- `src/tool/websearch.rs` and `src/tool/webfetch.rs` now call the backend dispatch layer.
- `src/config/schema.rs` has `SearchConfig`, `SearchBackendConfig`, and `EggsearchConfig`.
- `AgentLoop::build_tool_definitions` filters raw `mcp__eggsearch__*` tools unless `expose_raw_mcp_tools = true`.
- Tests now cover MCP dispatch mapping and basic raw-tool filtering logic.

Remaining issues are mostly edge-case correctness, test strength, and default semantics.

## Non-goals

Do not add new search providers in Codegg. New providers belong in eggsearch.

Do not reintroduce first-class raw MCP search tools as the normal agent surface.

Do not expand the legacy built-in search/fetch implementation. It is fallback-only.

Do not add a local index, crawler, browser automation, or Tantivy path in Codegg.

Do not change the model-facing native tool names. They should remain `websearch` and `webfetch`.

## Phase 1: Fix default eggsearch bootstrap semantics

### Problem

`SearchConfig::backend()` defaults to `SearchBackendConfig::Eggsearch`, and `EggsearchConfig` has sensible defaults:

```rust
command = "eggsearch"
args = ["mcp", "stdio"]
server_name = "eggsearch"
timeout_ms = 60_000
```

However, `bootstrap_eggsearch` currently returns early when `effective.eggsearch` is `None`:

```rust
let egg_cfg = match effective.eggsearch.as_ref() {
    Some(cfg) => cfg,
    None => {
        report.note = Some("no [search.eggsearch] section configured".to_string());
        return report;
    }
};
```

This means the backend defaults to eggsearch, but Codegg does not actually spawn `eggsearch mcp stdio` unless the user explicitly adds `[search.eggsearch]`. That conflicts with the intended default behavior.

### Required change

Change bootstrap so missing `[search.eggsearch]` means “use default eggsearch config,” not “do not configure eggsearch.”

Suggested implementation:

```rust
let egg_cfg = effective.eggsearch.clone().unwrap_or_default();
if egg_cfg.enabled == Some(false) {
    report.note = Some("[search.eggsearch] enabled = false".to_string());
    return report;
}
let server_name = egg_cfg.server_name().to_string();
```

Because this creates an owned `EggsearchConfig`, adjust downstream references accordingly.

### Acceptance criteria

With no `[search]` section at all, `bootstrap_search_backend` should report:

- backend: `eggsearch`
- command: `eggsearch mcp stdio`
- server name: `eggsearch`
- either connected, or unavailable with a spawn/initialize error if the binary is missing

It should not report `no [search.eggsearch] section configured` unless the backend is not eggsearch and that note is still meaningful.

### Tests

Add or update tests in `src/search_backend/bootstrap.rs`:

```rust
#[test]
fn default_search_config_uses_default_eggsearch_config() {
    let cfg = Config::default();
    let effective = effective_search_config(&cfg);
    assert_eq!(effective.backend(), SearchBackendConfig::Eggsearch);
    let egg = effective.eggsearch.clone().unwrap_or_default();
    assert_eq!(egg.server_name(), "eggsearch");
    assert_eq!(egg.command(), "eggsearch");
    assert_eq!(egg.args(), vec!["mcp", "stdio"]);
}
```

Add an async bootstrap test if feasible using a deliberately invalid command:

```toml
[search]
backend = "eggsearch"

[search.eggsearch]
command = "definitely-missing-eggsearch-test-binary"
```

Expected: report backend is eggsearch, command is the missing command, and `connection_error` is set. This proves missing binary is diagnosed after attempting bootstrap, not skipped.

## Phase 2: Make output truncation UTF-8 safe

### Problem

`src/search_backend/framing.rs::clamp_output` slices strings by byte index:

```rust
truncated.push_str(&content[..max_chars]);
```

This can panic if `max_chars` lands inside a multibyte UTF-8 sequence.

The legacy built-in fetch path also slices with:

```rust
&result[..max_length]
```

This has the same panic risk.

### Required change

Add a small helper for UTF-8-safe truncation.

Suggested implementation in `framing.rs`:

```rust
pub fn truncate_utf8_boundary(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }
    let mut end = 0;
    for (idx, _) in content.char_indices() {
        if idx > max_bytes {
            break;
        }
        end = idx;
    }
    if end == 0 {
        ""
    } else {
        &content[..end]
    }
}
```

A slightly better version should include the character whose starting byte is below the limit only if its full end byte is within the limit. A simpler safe implementation is:

```rust
pub fn truncate_utf8_boundary(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }
    let mut end = 0;
    for (idx, ch) in content.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    &content[..end]
}
```

Use this in `clamp_output`.

Also reuse it in `src/tool/webfetch.rs::process_response` for the legacy fallback path.

### Acceptance criteria

No code path slices arbitrary web output at a byte offset without checking a UTF-8 boundary.

### Tests

Add tests:

```rust
#[test]
fn clamp_output_handles_multibyte_boundary() {
    let s = "abcé日本語";
    let out = clamp_output(s, 4, "cap");
    assert!(out.contains("abc"));
    assert!(out.contains("[truncated by Codegg"));
}

#[test]
fn truncate_utf8_boundary_never_panics_on_emoji() {
    let s = "hello 🚀 world";
    for n in 0..s.len() {
        let _ = truncate_utf8_boundary(s, n);
    }
}
```

For the legacy fetch helper, at minimum add a unit-level test against the truncation helper rather than trying to mock a full HTTP response.

## Phase 3: Strengthen raw MCP exposure tests against the real agent path

### Problem

There are tests that reimplement the raw MCP filter predicate locally, but they do not directly exercise `AgentLoop::build_tool_definitions`.

That leaves a risk that future changes drift between tests and implementation.

### Required change

Add a direct integration test that constructs an `AgentLoop` with a mock `McpService` containing eggsearch tools, then verifies the actual tool definitions sent to the model.

`build_tool_definitions` is private. Choose one of these approaches:

Option A, preferred: add a `#[cfg(test)]` helper on `AgentLoop`:

```rust
#[cfg(test)]
pub async fn test_build_tool_definitions(&mut self) -> Vec<crate::provider::ToolDefinition> {
    self.build_tool_definitions().await
}
```

Option B: expose a narrow internal helper for filtering MCP tools:

```rust
pub(crate) fn filter_raw_eggsearch_mcp_tools(
    tools: Vec<ToolDefinition>,
    cfg: &SearchConfig,
) -> Vec<ToolDefinition>
```

Then both `AgentLoop::build_tool_definitions` and tests use the same helper. This is acceptable and may be cleaner.

### Acceptance criteria

A test proves that with:

```toml
[search]
backend = "eggsearch"
expose_raw_mcp_tools = false
```

The agent-facing tool definitions include:

- `websearch`
- `webfetch`

And do not include:

- `mcp__eggsearch__web_search`
- `mcp__eggsearch__web_fetch`
- `mcp__eggsearch__provider_status`

A second test proves that with `expose_raw_mcp_tools = true`, the raw MCP tools appear in addition to native wrappers.

### Notes

Keep `provider_status` raw exposure hidden by default. It can be surfaced later as a native diagnostic command/tool if needed.

## Phase 4: Verify bootstrap coverage across all execution modes

### Problem

`run_single_shot` calls `bootstrap_search_backend` before constructing the `AgentLoop`.

TUI non-socket startup calls `bootstrap_search_backend` before in-process agent execution.

However, daemon/core execution paths may construct an `AgentLoop` in a different place. The search backend state is process-global, so every process that can execute tools must bootstrap search before agent execution.

### Required investigation

Search for every call to:

```rust
AgentLoop::new(
```

For each call site, verify that the same process has already called:

```rust
search_backend::bootstrap::bootstrap_search_backend(&config).await
```

before the first turn can execute.

Likely places to inspect:

- `run_single_shot`
- `cmd_exec`
- TUI inproc execution
- TUI stdio core transport
- `core-stdio` handler
- daemon `TurnSubmit` handler
- `InprocCoreClient`
- `SocketCoreClient` daemon implementation
- subagent worker pool, if subagents can use `websearch`/`webfetch`

### Required change

Add bootstrap at the layer that owns agent execution, not only at the UI layer.

The safest architecture is:

- UI may call bootstrap for in-process convenience.
- Core process must also call bootstrap before agent execution.
- Bootstrap must remain idempotent.

If `AgentLoop` construction is centralized, place it immediately before `AgentLoop::new` and pass the returned `mcp_service` into the loop.

If a call site already has an `mcp_service`, ensure it is also installed in `search_backend::state` or used consistently by wrapper tools.

### Acceptance criteria

Every execution mode that can call `websearch` or `webfetch` has initialized search backend state.

If eggsearch is unavailable, the user gets a clear tool error at call time, not a silent missing-tool or stale state failure.

### Tests

Add a unit/integration test at the most central core execution path if feasible.

At minimum, add a comment near each `AgentLoop::new` call documenting how search backend bootstrap is satisfied.

## Phase 5: Remove or wire dead timeout fields

### Problem

`WebSearchTool` still has `timeout_secs` and `with_timeout`, but `execute()` ignores the field because dispatch handles backend calls.

```rust
pub struct WebSearchTool {
    timeout_secs: u64,
}
```

This is confusing and may mislead future maintainers.

### Required change

Choose one:

Option A, preferred for now: remove `timeout_secs` and `with_timeout` from `WebSearchTool`.

Option B: plumb timeout through `dispatch_web_search` and `eggsearch::call_web_search`.

Given the current config already has `EggsearchConfig.timeout_ms`, Option A is cleaner.

### Acceptance criteria

No unused timeout field remains in `WebSearchTool`.

If callers/tests used `with_timeout`, update them or remove the test path.

## Phase 6: Tighten fallback semantics and documentation

### Problem

The legacy built-in fetch path is intentionally retained, but it remains broader and less aligned with eggsearch’s security posture. It can return base64 image attachments and has different extraction behavior.

This is acceptable only if fallback remains explicit and documented.

### Required change

Ensure documentation states:

- Default backend is eggsearch.
- Built-in backend is legacy compatibility fallback.
- `fallback_to_builtin = false` by default.
- Built-in fetch behavior may differ from eggsearch and should not be considered the preferred security boundary.
- New providers should be added in eggsearch, not Codegg.

Update any config examples to show the recommended configuration:

```toml
[search]
backend = "eggsearch"
expose_raw_mcp_tools = false
fallback_to_builtin = false
max_search_output_chars = 12000
max_fetch_output_chars = 20000

[search.eggsearch]
# Optional. Defaults shown here for clarity.
enabled = true
server_name = "eggsearch"
command = "eggsearch"
args = ["mcp", "stdio"]
timeout_ms = 60000
```

Also document the minimal configuration:

```toml
# No [search] section is required if eggsearch is installed on PATH.
# Codegg defaults to spawning: eggsearch mcp stdio
```

This minimal statement is only valid after Phase 1 fixes default bootstrap.

### Acceptance criteria

Docs and actual defaults agree.

There is no doc path suggesting that built-in search is the preferred new-provider path.

## Phase 7: Validate provider hint mapping against actual eggsearch capabilities

### Problem

`translate_provider_hint` maps many historical Codegg provider hints to eggsearch provider IDs:

- `wikipedia`
- `arxiv`
- `openalex`
- `pubmed`
- `hn_algolia`
- `google_news`
- `github`
- `exa`
- `tavily`
- `kagi`
- `serpapi`

This is fine only if eggsearch actually supports those IDs, or if unknown provider IDs fail gracefully.

### Required change

Check eggsearch’s current provider IDs. Update Codegg’s `provider` enum and mapping to include only providers that eggsearch supports today, unless the adapter intentionally treats unsupported historical hints as `auto`.

Recommended conservative behavior:

- Keep `auto`, `duckduckgo`, `mojeek`, `brave`/`brave_api`, `searxng` if supported.
- For historical providers not supported by eggsearch yet, map to `auto` or return a clear error.

Avoid sending unsupported provider IDs to eggsearch if eggsearch treats unknown providers as errors.

### Acceptance criteria

The model-facing `provider` enum in `websearch.parameters()` does not advertise provider hints that eggsearch cannot satisfy under the default backend.

Alternatively, the tool description explicitly says some provider hints require `[search].backend = "builtin"` or future eggsearch providers. This is less ideal.

### Suggested test

Add tests for `translate_provider_hint`:

```rust
assert_eq!(translate_provider_hint(Some("duckduckgo")), Some(vec!["duckduckgo".into()]));
assert_eq!(translate_provider_hint(Some("mojeek")), Some(vec!["mojeek".into()]));
assert_eq!(translate_provider_hint(Some("brave")), Some(vec!["brave_api".into()]));
assert_eq!(translate_provider_hint(Some("unsupported_historical")), Some(vec![]));
```

If `translate_provider_hint` remains private, test through a small public(crate) helper or keep focused unit tests inside `eggsearch.rs`.

## Phase 8: Check global state test isolation

### Problem

`search_backend::state` uses process-global mutable slots behind `StdRwLock`. Tests install config and MCP service into global state.

The fake MCP test serializes itself with a local mutex, but tests in other modules can still mutate the same global state in parallel.

### Required change

Add test-only reset helpers:

```rust
#[cfg(test)]
pub fn reset_for_tests() {
    *MCP_SERVICE.write().unwrap() = None;
    *SEARCH_CONFIG.write().unwrap() = None;
}
```

Call this at the start/end of tests that mutate global search backend state.

If Rust test parallelism still causes cross-file interference, consider using the `serial_test` crate or consolidate search backend global-state tests into one serialized module.

### Acceptance criteria

Search backend tests do not depend on test execution order or stale global state.

The permissive test language around “stale McpService from previous test” should no longer be necessary.

## Phase 9: Update plan/docs status and remove stale plan caveats

### Required change

Update `plans/eggsearch.md` or archive it as completed if it still reads like a future implementation plan.

Add or update architecture docs, likely `architecture/search_backend.md`, to describe the final intended state:

- Native tool names are stable.
- Eggsearch owns web provider/fetching logic.
- Codegg owns wrapper UX, permissioning, output caps, trust framing, and backend selection.
- Built-in search is legacy fallback only.
- Raw MCP tools are hidden unless explicitly requested.

### Acceptance criteria

A new contributor can read the docs and correctly answer:

- Why do `websearch` and `webfetch` still exist in Codegg?
- Why does Codegg also have generic MCP support?
- Why are raw eggsearch MCP tools hidden by default?
- Where should a new web search provider be added?
- What happens if eggsearch is missing?

## Final acceptance checklist

The final pass is complete when all of these are true:

- With no `[search]` config, Codegg attempts to use `eggsearch mcp stdio` by default.
- If eggsearch is missing, `websearch`/`webfetch` return an actionable eggsearch error.
- If `[search].backend = "builtin"`, native tools use the legacy in-tree implementation.
- If `[search].backend = "disabled"`, native tools error clearly and do not call MCP.
- If `[search].fallback_to_builtin = true`, eggsearch failure falls back to built-in behavior.
- `websearch` maps `num_results` to eggsearch `max_results`.
- `webfetch` maps `max_length` to eggsearch `max_chars`.
- Eggsearch search and fetch outputs are framed as `external_untrusted`.
- Raw `mcp__eggsearch__*` tools are hidden from model-facing tools by default.
- Raw `mcp__eggsearch__*` tools are visible only when `expose_raw_mcp_tools = true`.
- Output truncation is UTF-8 safe.
- Every core execution path that can run agent tools bootstraps the search backend.
- Docs describe eggsearch as the native backend and the built-in provider stack as legacy fallback.

## Recommended implementation order

1. Fix default eggsearch bootstrap semantics.
2. Add UTF-8-safe truncation helper and replace unsafe slices.
3. Audit and patch bootstrap coverage across all `AgentLoop::new` call sites.
4. Remove unused `WebSearchTool.timeout_secs` or wire it properly.
5. Strengthen raw MCP exposure tests against the real implementation.
6. Add state reset helpers for tests.
7. Validate provider hint mapping against eggsearch’s actual provider IDs.
8. Update docs and config examples.
9. Run the full test suite.

## Suggested validation commands

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Manual checks:

```bash
# With eggsearch installed
codegg doctor --subsystem search

# With eggsearch temporarily hidden from PATH
PATH=/usr/bin:/bin codegg doctor --subsystem search

# Built-in fallback mode
# config: [search] backend = "builtin"
codegg doctor --subsystem search

# Disabled mode
# config: [search] backend = "disabled"
codegg --run "search the web for rust async cancellation"
```

Expected manual behavior:

- Installed eggsearch: doctor shows connected and tools include `web_search`, `web_fetch`, `provider_status`.
- Missing eggsearch: doctor shows unavailable but Codegg starts.
- Missing eggsearch plus `backend = "eggsearch"`: `websearch` returns actionable eggsearch unavailable error.
- `backend = "builtin"`: `websearch` uses the legacy provider registry.
- `backend = "disabled"`: `websearch` and `webfetch` fail clearly without calling MCP.

