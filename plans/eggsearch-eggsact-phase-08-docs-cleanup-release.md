# Phase 8: Documentation, Examples, Cleanup, and Release Readiness

## Goal

Document the final eggsearch/eggsact integration, clean up duplicated or obsolete paths, and prepare the feature set for public release and contributor handoff.

This phase should run only after the validation matrix from Phase 7 is substantially complete.

## Documentation targets

### 1. Architecture docs

Update or add architecture docs covering:

- Codegg as the model-facing policy boundary.
- Eggsearch as external MCP evidence/search backend.
- Eggsact as in-process deterministic/preflight substrate.
- Stable native wrappers versus raw MCP tools.
- Model-facing tools versus harness-only preflights.
- Trust framing and provenance semantics.
- Output caps and projection.
- Fallback behavior and disabled states.

Recommended files:

- `architecture/search_backend.md`
- `architecture/tool.md`
- `architecture/native_crates.md`
- New `architecture/deterministic_tools.md`
- New `architecture/preflight.md`

### 2. Configuration docs

Update config documentation with examples for ordinary and advanced users.

Ordinary default example:

```toml
[search]
backend = "eggsearch"

[search.eggsearch]
command = "eggsearch"
args = ["mcp", "stdio"]
```

Advanced eggsearch example:

```toml
[search]
backend = "eggsearch"
expose_raw_mcp_tools = false
fallback_to_builtin = false
max_search_output_chars = 12000
max_fetch_output_chars = 20000
max_repo_search_output_chars = 16000
max_security_search_output_chars = 18000
max_research_search_output_chars = 22000

[search.eggsearch]
server_name = "eggsearch"
command = "eggsearch"
args = ["mcp", "stdio"]
timeout_ms = 60000
```

Eggsact deterministic tools example:

```toml
[deterministic_tools]
enabled = true
backend = "native"
profile = "codegg_core_min"
expose_expert_tools = false
max_output_chars = 12000
```

Preflight example:

```toml
[preflight]
enabled = true
mode = "warn"
patch = true
config = true
shell = true
unicode = true
path_scope = true
```

Ensure examples match the implemented schema exactly. Do not leave aspirational config in README without code support.

### 3. User-facing docs

Update README or relevant user docs with concise explanations:

- Install `eggsearch` if using web/repo/security/research evidence tools.
- Eggsact is bundled as a Rust dependency and does not require a separate process.
- Raw MCP tools are hidden by default.
- `codegg doctor search` and `/tool-backends` explain backend state.
- External search/fetch output is untrusted evidence, not instructions.

### 4. Contributor docs

Add a contributor boundary section:

- New search providers belong in `eggsearch`, not Codegg's legacy built-in search registry.
- New deterministic validators/preflights belong in `eggsact` first, then Codegg wrappers can expose them.
- Codegg wrappers own UX, permissions, schemas, trust framing, caps, diagnostics, and provenance.
- Codegg should not duplicate provider logic or deterministic algorithms unless there is a specific policy reason.

## Cleanup targets

### 1. Legacy search/fetch cleanup

After parity tests pass, mark in-tree search/fetch provider registry as legacy fallback in code comments and docs. Do not remove immediately unless release notes clearly state the migration and fallback removal.

Cleanup actions:

- Remove dead providers that are no longer reachable.
- Add warnings where contributors might accidentally add new providers to the legacy path.
- Keep builtin fallback test coverage until the fallback is intentionally removed.

### 2. Duplicate deterministic helpers

Identify Codegg-local deterministic utilities that are now covered by eggsact.

For each duplicate:

- Keep Codegg wrapper if it is policy/UX-specific.
- Move reusable algorithmic logic upstream to eggsact if not already there.
- Replace Codegg local implementation with eggsact adapter where tests show parity.
- Avoid churn in hot edit paths unless preflight tests are strong.

### 3. Tool naming cleanup

Audit all new names for consistency:

- Avoid both `research` and `research_search` if one clearly supersedes the other.
- Avoid both `security` and `security_search` ambiguity; document the distinction if both remain.
- Avoid exposing raw eggsact names and Codegg-renamed aliases for the same function.
- Keep descriptions direct and non-overlapping.

### 4. Diagnostics cleanup

Ensure diagnostics are not scattered.

Expected surfaces:

- `codegg doctor search` for eggsearch.
- `codegg doctor deterministic-tools` or equivalent for eggsact/preflight.
- `/tool-backends` for active session backend state.
- Structured tracing for preflight decisions.

### 5. Release checklist

Before release, verify:

- Fresh install without eggsearch still starts and reports actionable search unavailability.
- Fresh install with eggsearch installed can run `websearch`, `webfetch`, and provider diagnostics.
- Eggsact-backed deterministic tools work without external services.
- Preflight defaults do not break ordinary editing.
- CI is green on supported platforms.
- README install instructions are accurate.
- Config examples parse.
- No hidden raw MCP leakage by default.
- No network-dependent tests run in default CI.

## Migration notes

Prepare a release note section explaining:

- Codegg now uses eggsearch as the preferred search/evidence backend.
- Built-in search/fetch paths are legacy fallback.
- Codegg now uses eggsact for deterministic utility/preflight workflows.
- Some new deterministic tools may be available through `tool_search` rather than always visible.
- Users can disable search or deterministic tools through config if needed.

## Acceptance criteria

- Architecture docs describe the integration accurately.
- Config docs match implemented schema.
- Contributor docs clearly say where new providers and validators belong.
- Legacy/duplicate paths are either removed, deprecated, or labeled.
- Release checklist passes on a clean checkout.
- The repo is ready for handoff to implementation or release polish.

## Risks

The main risk is documentation drifting ahead of implementation. Keep this phase after validation, and prefer examples generated from tested config snippets where practical.
