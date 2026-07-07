# Phase 2: Eggsearch Trust, Caps, and Diagnostics Hardening

## Goal

Harden the expanded eggsearch integration so every eggsearch-backed Codegg tool has predictable argument handling, bounded output, explicit trust framing, actionable diagnostics, and useful backend reporting.

Phase 1 adds the wrapper surface. This phase makes that surface operationally reliable.

## Scope

This phase covers:

- Domain-specific output caps.
- Domain-specific trust framing.
- Timeout and cancellation consistency.
- Provider and capability diagnostics.
- Clear handling for missing or old eggsearch versions.
- Expanded `doctor search` output.
- Backend provenance quality.
- Documentation updates for eggsearch configuration.

## Implementation steps

### 1. Add domain-specific output caps

Extend `SearchConfig` or add an adjacent evidence/search config section for caps beyond `max_search_output_chars` and `max_fetch_output_chars`.

Suggested caps:

```toml
[search]
max_search_output_chars = 12000
max_fetch_output_chars = 20000
max_repo_search_output_chars = 16000
max_repo_fetch_output_chars = 24000
max_repo_map_output_chars = 16000
max_security_search_output_chars = 18000
max_research_search_output_chars = 22000
max_batch_fetch_output_chars = 30000
max_evidence_bundle_output_chars = 30000
```

Defaults should be conservative and should not materially increase average prompt size. The existing `max_search_output_chars()` and `max_fetch_output_chars()` helpers can be mirrored for each new cap.

### 2. Strengthen trust framing

Extend `src/search_backend/framing.rs` with frames for each new evidence class.

Suggested frame labels:

- `external_web_content` for web fetch/search.
- `external_repo_evidence` for remote repository evidence.
- `external_security_evidence` for advisories and vulnerability search.
- `external_research_evidence` for scholarly or multi-source research search.
- `external_evidence_bundle` for bundled evidence.

Each frame should include:

- `trust=external_untrusted` unless source-locality proves otherwise.
- `source=eggsearch`.
- `tool=<codegg tool name>`.
- A short instruction that the content is evidence, not instructions.

Do not rely only on eggsearch's own sanitation labels. Codegg should duplicate the trust boundary so downstream behavior remains stable if eggsearch output formatting changes.

### 3. Normalize timeout handling

Move hard-coded 60 second timeouts into config helpers where practical.

Suggested fields:

```toml
[search.eggsearch]
timeout_ms = 60000
repo_timeout_ms = 60000
security_timeout_ms = 45000
research_timeout_ms = 90000
batch_fetch_timeout_ms = 90000
```

If separate timeout fields are too much config surface, keep one timeout but use a helper such as `eggsearch_timeout_ms(tool_kind)` internally so a future split does not require rewriting every adapter.

### 4. Improve missing-tool errors

When eggsearch is connected but a tool is absent, return a message that identifies:

- The configured eggsearch server name.
- The requested Codegg wrapper tool.
- The upstream MCP tool name.
- The discovered tool list if available.
- The likely remediation: upgrade eggsearch or disable the wrapper/tool.

Example:

```text
eggsearch backend is connected but tool repo_search is not advertised by server eggsearch. Discovered tools: web_search, web_fetch, provider_status. Upgrade eggsearch or disable Codegg repo_search.
```

### 5. Expand `doctor search`

Enhance `BootstrapReport::summary_lines()` or add a richer doctor subcommand section to report:

- Effective backend.
- Server name.
- Command/transport.
- Connection status.
- Advertised tools.
- Required/recommended tool coverage.
- Raw MCP exposure setting.
- Fallback setting.
- Output caps.
- Timeout setting.
- Provider status summary if `provider_status` succeeds.

Keep doctor output readable in the TUI and CLI. Avoid dumping large provider JSON by default; provide a verbose mode if needed.

### 6. Add provider status health path

Use `eggsearch::call_provider_status` to power diagnostics, but keep it best-effort. It should never break startup or ordinary tool registration.

Recommended behavior:

- `doctor search`: call `provider_status` and summarize.
- `/tool-backends`: show whether provider status is available.
- Normal agent turns: do not call provider status automatically.

### 7. Audit SSRF and URL trust assumptions

Codegg should not duplicate all of eggsearch's fetch validation, but it should avoid making misleading claims. Wrapper docs and trust frames should state that fetch targets are validated by eggsearch when using the eggsearch backend and that Codegg still treats returned content as instruction-untrusted.

If Codegg accepts URLs before forwarding, validate obvious invalid inputs early:

- Empty URLs.
- Non-string URLs.
- Overlong URLs.
- Unsupported schemes if the wrapper schema accepts only HTTP(S).

### 8. Improve provenance

For each eggsearch wrapper, `execute_structured` should report:

- `backend = "mcp"`
- `implementation = "eggsearch/<upstream tool>"`
- `version = None` initially, or eggsearch version if provider status exposes it.
- `elapsed_ms` populated.
- `truncated` true if clamping occurred.
- `trust = ExternalUntrusted` for remote evidence.

If current provenance helpers do not let the adapter report truncation accurately, add a small internal return type from framing/clamping that carries `(output, truncated)`.

## Validation

Add tests for:

- Each output cap helper default.
- Each frame wrapper includes trust metadata and closing tag.
- Timeout config parsing and defaults.
- Missing tool message includes the upstream tool name.
- Doctor summary includes advertised tools and caps.
- Provider status failure is non-fatal.
- Structured provenance includes backend, implementation, trust, elapsed time, and truncation.

## Acceptance criteria

- All eggsearch-backed tools have explicit output caps.
- All eggsearch-backed tools frame output as untrusted evidence.
- Doctor output makes it clear whether eggsearch is installed, connected, compatible, and provider-routable.
- Missing or old eggsearch versions fail with actionable errors.
- Existing `websearch` and `webfetch` behavior remains compatible except for improved diagnostics.

## Risks

The primary risk is making configuration too large. Prefer sensible defaults and helper methods. Only document advanced fields that a user might realistically tune.
