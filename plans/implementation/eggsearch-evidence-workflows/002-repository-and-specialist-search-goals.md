# Eggsearch Evidence Workflows M002 — Repository and Specialist Search Goals

Status: blocked (ready after legacy-retirement M001 closes)
Class: capability / compatibility
Planning baseline: CodeGG main `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
Source roadmap: `plans/subsystems/eggsearch-evidence-workflows-roadmap.md`
References: `plans/000-long-term-specification.md`; `plans/003-planning-process.md`; `plans/closure/tool-surface-upstream-compatibility/007-status.md`.
Hard dependency: `plans/implementation/search-eggsearch-legacy-retirement/001-eggsearch-parity-adoption-and-provider-contract.md` **strictly closed**, with an installed qualified upstream binary and captured tool schemas. M001 evidence-fidelity work may run concurrently, but do not share a branch or race overlapping edits.

## 1. Objective

Give CodeGG's coding agents native, provider-neutral ways to request task-aware repository discovery and specialist research based on the *installed* eggsearch 0.4.x tool contract. Minimize incorrect generic search, silent argument omission and unnecessary follow-up calls.

## 2. Evidence and scope

- `src/tool/repo_search.rs` currently exposes `query, repo, owner, host, language, path, file, symbol, profile, include_local, mode, max_results`; adapter `src/search_backend/eggsearch.rs` forwards those legacy fields.
- Upstream `repo_search` supports `goal` values such as `understand`, `architecture`, `debug`, `migration`, `security`, `dependency`, `performance`, `compare`, `pre_change`, `post_change`, and explicit `sources` families: `code`, `docs`, `registry`, `issues`, `pull_requests`, `releases`, `examples`, `changelog`, `migration_guides`, `security`. Reconfirm exact release schemas; do not assume unreleased `main` fields exist in the pin.
- Audit `research_search` and `security_search` actual schemas and CodeGG wrappers before adding any `goal` / `include` / `workflow` translations. Never assume a common schema across distinct tools.
- Native `grep`, `glob`, LSP, `codesearch` alias and Git local tools continue to own cheap local inspection. Eggsearch searches complementary upstream ecosystems.

## 3. Functional requirements

1. Extend the native `repo_search` schema with upstream-recognized task `goal` and `sources`. Validate enum and list bounds against *pinned* inputSchema, including empty arrays, unknown values, and duplicate sources; do not silently drop explicit selectors.
2. Preserve legacy `profile`/`mode` requests only where mappings are semantically lossless. Write an explicit compatibility matrix (e.g., `exact_error` may not mean `debug`). If semantics differ, reject with actionable guidance and plan a documented removal; do not force-map every legacy value.
3. Translate native inputs exactly once in `src/search_backend/eggsearch.rs`; a typed argument DTO/helper is preferred over multiple independent wrapper mappings. Do not add a provider-side query planner or create new GitHub API clients in CodeGG.
4. For `research_search` and `security_search`, promote only high-value current fields after independent schema audit: task/context intent, include/source filters, repository anchoring, and result caps where supported. Keep distinct semantics instead of exposing an accidental cross-tool super-schema.
5. Preserve structured `retrieval_summary`, source provenance, warnings and typed errors in the internal call result; model-visible output remains limited and untrusted. Unsupported explicit constraints fail, rather than broadening search.
6. In `tool_search` descriptions and role-aware immediate/deferred catalog, add concise guidance connecting coding scenarios to the right specialist tool: architecture, dependency upgrades, security/CVEs, before/after change review. Do not make all specialist tools immediately visible.

## 4. Ordered work packages

- WP1: snapshot installed `tools/list` input and output schemas from exact pinned release; table CodeGG schema -> upstream -> keep/map/reject/defer for repo/research/security.
- WP2: add strictly validated modern `repo_search.goal`/`sources` native arguments; implement translators and explicit alias behavior.
- WP3: audit/add chosen specialist controls based on actual release; extend tests to ensure no silently ignored request fields.
- WP4: improve tool descriptions and documentation, preserving existing CodeGG progressive disclosure.
- WP5: local binary smoke for `repo_search` with new controls and invalid selectors (offline test by schema or mock where network is not required). Record missing-provider outcomes as expected rather than attempt live network CI.

## 5. Compatibility, security and resource semantics

No wire protocol, durable storage, config or permission migration anticipated. Model/tool arguments are untrusted; host/repo URLs continue through eggsearch SSRF and CodeGG existing policy. `include_local` cannot cross a workspace boundary or grant file read authority. Explicit provider/source restriction cannot degrade to auto/broader selection when unavailable. Preserve existing MCP timeout, call limits and cancellation; no search fan-out in CodeGG.

## 6. Verification

```bash
cargo fmt --all -- --check
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Fixture tests MUST assert accepted goal/source names, exclusions, invalid enum/list inputs, legacy alias parity/rejection, specialist current-schema controls, positive response and `isError` failures, and stable tool definitions. Include at least two end-to-end native-tool-to-fake-MCP trajectories (repository architecture and dependency/migration), plus negative cases that no network is performed for invalid selectors.

## 7. Acceptance and stop conditions

**Accept:** modern goals/sources reach the exact upstream keys for supported release; invalid/obsolete inputs fail actionably; no extra CodeGG provider backend or local-tool regression; role disclosure remains narrow; focused and quick checks pass.

**Stop:** if tagged release lacks essential goal/source semantics or aliases cannot be preserved without semantic broadening, document discrepancy and update this plan before changing public tool schema.

Closure: `plans/closure/eggsearch-evidence-workflows/002-status.md`: pin/schema fingerprint, alias matrix, fixtures, tool-call evidence, no-op/rejection cases, tests, security/cancellation, remaining risks.
